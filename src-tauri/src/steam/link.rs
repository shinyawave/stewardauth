// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Create (link) a brand-new Steam Mobile authenticator from scratch.
//!
//! We build our OWN AddAuthenticator/FinalizeAddAuthenticator requests via
//! steamguard's public `TwoFactorClient` + protobufs, because
//! `steamguard::AccountLinker` hardcodes SMS (`sms_phone_id`/`validate_sms_code`)
//! and cannot do email-only linking. See the design spec.

use base64::{engine::general_purpose::STANDARD, Engine};
use steamguard::protobufs::service_twofactor::{
    CTwoFactor_AddAuthenticator_Request, CTwoFactor_AddAuthenticator_Response,
    CTwoFactor_FinalizeAddAuthenticator_Request,
};
use steamguard::protobufs::steammessages_auth_steamclient::{
    EAuthSessionGuardType, EAuthTokenPlatformType,
};
use steamguard::steamapi::{EResult, TwoFactorClient};
use steamguard::token::Tokens;
use steamguard::transport::WebApiTransport;
use steamguard::userlogin::{DeviceDetails, LoginError, UserLogin};
use crate::steam::totp::generate_steam_code;
use crate::vault::model::{Account, Proxy};

/// Where Steam sends the AddAuthenticator confirmation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmType {
    Sms,
    Email,
}

impl ConfirmType {
    /// Steam's `confirm_type`: 1/2 = phone (SMS), 3 = email. Anything else → Email
    /// (safer: the email path does not `validate_sms_code`).
    pub fn from_i32(i: i32) -> ConfirmType {
        match i {
            1 | 2 => ConfirmType::Sms,
            _ => ConfirmType::Email,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfirmType::Sms => "sms",
            ConfirmType::Email => "email",
        }
    }
}

/// True when Steam requires an emailed Guard code to finish this login.
/// Takes the guard types from `begin_auth_via_credentials`' allowed confirmations.
pub fn needs_email_guard(guard_types: &[EAuthSessionGuardType]) -> bool {
    guard_types
        .iter()
        .any(|g| *g == EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode)
}

/// The secrets produced by a successful AddAuthenticator, ready to persist.
#[derive(Debug, Clone)]
pub struct LinkedData {
    /// Our on-disk account model (secrets base64-encoded).
    pub account: Account,
    /// Raw shared-secret bytes — needed to generate the finalize TOTP.
    pub shared_secret_bytes: Vec<u8>,
    pub server_time: u64,
    pub confirm_type: ConfirmType,
    pub phone_hint: String,
}

/// Outcome of an AddAuthenticator call.
#[derive(Debug)]
pub enum AddOutcome {
    /// Success — proceed to entering the SMS/email code.
    Code(LinkedData),
    /// Account has no verified phone; must attach one first.
    NeedPhone,
    /// Account already has an authenticator (use Import instead).
    AlreadyLinked,
    RateLimited,
    /// Unexpected Steam error (message for logging, not shown verbatim).
    Failed(String),
}

/// Map the AddAuthenticator response into an outcome. Pure — unit-testable by
/// constructing a protobuf response and passing a chosen `EResult`.
pub fn map_add_response(
    result: EResult,
    resp: &mut CTwoFactor_AddAuthenticator_Response,
    device_id: &str,
    steam_id: u64,
) -> AddOutcome {
    if result != EResult::OK {
        return match result {
            // No phone on the account → must attach one (or Steam refused email).
            EResult::NoVerifiedPhone | EResult::Fail => AddOutcome::NeedPhone,
            EResult::DuplicateRequest => AddOutcome::AlreadyLinked,
            EResult::RateLimitExceeded => AddOutcome::RateLimited,
            other => AddOutcome::Failed(format!("{other:?}")),
        };
    }
    // Belt-and-suspenders: some responses carry status 29 (duplicate) with OK.
    if resp.status() == 29 {
        return AddOutcome::AlreadyLinked;
    }

    let shared_bytes = resp.take_shared_secret();
    let account = Account {
        steam_id: steam_id.to_string(),
        account_name: resp.take_account_name(),
        shared_secret: STANDARD.encode(&shared_bytes),
        identity_secret: STANDARD.encode(resp.take_identity_secret()),
        device_id: device_id.to_string(),
        revocation_code: resp.take_revocation_code(),
    };
    AddOutcome::Code(LinkedData {
        account,
        shared_secret_bytes: shared_bytes,
        server_time: resp.server_time(),
        confirm_type: ConfirmType::from_i32(resp.confirm_type()),
        phone_hint: resp.take_phone_number_hint(),
    })
}

/// Errors that abort the flow (as opposed to flow states carried in Ok values).
#[derive(Debug)]
pub enum LinkError {
    BadCredentials,
    RateLimited,
    Network(String),
}

impl From<LoginError> for LinkError {
    fn from(e: LoginError) -> Self {
        match e {
            LoginError::BadCredentials => LinkError::BadCredentials,
            LoginError::TooManyAttempts => LinkError::RateLimited,
            other => LinkError::Network(format!("{other:?}")),
        }
    }
}

fn transport(proxy: Option<&Proxy>) -> WebApiTransport {
    WebApiTransport::new(crate::steam::proxy::build_blocking_client(proxy))
}

/// A plausible Android device name so Steam's "Manage Devices" list shows a real
/// phone model instead of the app name. Picked pseudo-randomly (via a uuid byte)
/// so each linked authenticator looks like a distinct device.
fn random_android_device_name() -> String {
    const MODELS: &[&str] = &[
        "Galaxy S23", "Galaxy S22", "Galaxy S21", "Galaxy A54", "Galaxy Note 20",
        "Pixel 8", "Pixel 7", "Pixel 6a", "Redmi Note 12", "Xiaomi 13",
        "OnePlus 11", "OnePlus 9", "POCO X5", "Realme 11", "Moto G Power",
    ];
    let idx = (uuid::Uuid::new_v4().as_bytes()[0] as usize) % MODELS.len();
    MODELS[idx].to_string()
}

fn device_details() -> DeviceDetails {
    // platform/os values proven in steam/session.rs; friendly_name mimics a real
    // Android phone so Steam Guard's device list shows a phone, not the app name.
    DeviceDetails {
        friendly_name: random_android_device_name(),
        platform_type: EAuthTokenPlatformType::k_EAuthTokenPlatformType_MobileApp,
        os_type: -500,
        gaming_device_type: 1,
    }
}

/// The end result of the finalize loop, surfaced to the frontend.
#[derive(Debug, PartialEq, Eq)]
pub enum FinalizeResult {
    Done,
    /// The SMS/email confirmation code was wrong — ask the user again.
    WrongCode,
    /// Could not generate accepted 2FA codes (clock desync) after all retries.
    TimeSyncFailed,
}

/// Decide what to do after ONE finalize attempt. Pure — unit-testable.
/// `attempt` is 0-based; `max` is the retry cap (30).
#[derive(Debug, PartialEq, Eq)]
pub enum FinalizeStep {
    Done,
    Retry,
    WrongCode,
    GiveUp,
}

pub fn finalize_step(attempt: u32, max: u32, result: EResult, want_more: bool) -> FinalizeStep {
    if result == EResult::OK {
        if want_more {
            return if attempt + 1 < max { FinalizeStep::Retry } else { FinalizeStep::GiveUp };
        }
        return FinalizeStep::Done;
    }
    match result {
        // The confirmation code (SMS/email) was wrong — not retryable here.
        EResult::TwoFactorActivationCodeMismatch => FinalizeStep::WrongCode,
        // Our generated TOTP wasn't accepted (time desync) — retry with a fresh code.
        EResult::TwoFactorCodeMismatch => {
            if attempt + 1 < max { FinalizeStep::Retry } else { FinalizeStep::GiveUp }
        }
        _ => FinalizeStep::GiveUp,
    }
}

/// Result of starting the login. If `needs_email_guard`, keep `login` alive and
/// call [`submit_email_and_poll`] once the user provides the emailed code;
/// otherwise `tokens`/`steam_id` are already populated.
pub struct StartLoginResult {
    pub login: UserLogin<WebApiTransport>,
    pub needs_email_guard: bool,
    pub tokens: Option<Tokens>,
    pub steam_id: Option<u64>,
}

/// Begin credential login. Blocking. On no-guard accounts, polls tokens now.
pub fn start_login(
    proxy: Option<&Proxy>,
    username: &str,
    password: &str,
) -> Result<StartLoginResult, LinkError> {
    let mut login = UserLogin::new(transport(proxy), device_details());
    let confirmations = login.begin_auth_via_credentials(username, password)?;
    let guard_types: Vec<_> = confirmations.iter().map(|c| c.confirmation_type).collect();
    let needs = needs_email_guard(&guard_types);
    if needs {
        return Ok(StartLoginResult { login, needs_email_guard: true, tokens: None, steam_id: None });
    }
    // No emailed code required (fresh account with no Steam Guard, or device
    // confirmation) — poll straight through.
    let tokens = login.poll_until_tokens().map_err(|e| LinkError::Network(e.to_string()))?;
    let steam_id = decode_steam_id(&tokens)?;
    Ok(StartLoginResult { login, needs_email_guard: false, tokens: Some(tokens), steam_id: Some(steam_id) })
}

/// Submit the emailed Guard code and poll for tokens. Blocking.
pub fn submit_email_and_poll(
    mut login: UserLogin<WebApiTransport>,
    code: &str,
) -> Result<(Tokens, u64), LinkError> {
    login
        .submit_steam_guard_code(EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode, code.to_owned())
        .map_err(|e| match e {
            steamguard::userlogin::UpdateAuthSessionError::TooManyAttempts => LinkError::RateLimited,
            steamguard::userlogin::UpdateAuthSessionError::IncorrectSteamGuardCode => LinkError::BadCredentials,
            other => LinkError::Network(format!("{other:?}")),
        })?;
    let tokens = login.poll_until_tokens().map_err(|e| LinkError::Network(e.to_string()))?;
    let steam_id = decode_steam_id(&tokens)?;
    Ok((tokens, steam_id))
}

fn decode_steam_id(tokens: &Tokens) -> Result<u64, LinkError> {
    tokens
        .access_token()
        .decode()
        .map(|d| d.steam_id())
        .map_err(|e| LinkError::Network(format!("decode access token: {e}")))
}

/// Call our own AddAuthenticator (no `sms_phone_id`). Blocking.
pub fn add_authenticator(tokens: &Tokens, proxy: Option<&Proxy>) -> Result<AddOutcome, LinkError> {
    let steam_id = decode_steam_id(tokens)?;
    let device_id = format!("android:{}", uuid::Uuid::new_v4());
    let client = TwoFactorClient::new(transport(proxy));

    let mut req = CTwoFactor_AddAuthenticator_Request::new();
    req.set_authenticator_type(1);
    req.set_steamid(steam_id);
    req.set_device_identifier(device_id.clone());
    req.set_version(2);
    // NOTE: deliberately NOT setting sms_phone_id — that is the SMS bias we avoid.

    let resp = client
        .add_authenticator(req, tokens.access_token())
        .map_err(|e| LinkError::Network(e.to_string()))?;
    // NOTE: `resp.result` the FIELD is pub(crate); external callers must use the
    // public accessor `resp.result()`. Same for `into_response_data()`.
    let result = resp.result();
    let mut data = resp.into_response_data();
    Ok(map_add_response(result, &mut data, &device_id, steam_id))
}

/// Finalize: loop up to 30 times, each time fetch server time, generate a TOTP
/// from the raw shared secret, and POST finalize. Blocking.
pub fn finalize(
    tokens: &Tokens,
    steam_id: u64,
    shared_secret_bytes: &[u8],
    confirm_type: ConfirmType,
    code: &str,
    proxy: Option<&Proxy>,
) -> Result<FinalizeResult, LinkError> {
    const MAX: u32 = 30;
    let client = TwoFactorClient::new(transport(proxy));
    let validate_sms = confirm_type == ConfirmType::Sms;

    for attempt in 0..MAX {
        // Fresh server time each attempt (re-align with Steam's server clock).
        let time_resp = client.query_time().map_err(|e| LinkError::Network(e.to_string()))?;
        let server_time = time_resp.into_response_data().server_time() as u64;
        let totp = generate_steam_code(shared_secret_bytes, server_time);

        let mut req = CTwoFactor_FinalizeAddAuthenticator_Request::new();
        req.set_steamid(steam_id);
        req.set_authenticator_code(totp);
        req.set_authenticator_time(server_time);
        req.set_activation_code(code.to_owned());
        req.set_validate_sms_code(validate_sms);

        let resp = client
            .finalize_authenticator(req, tokens.access_token())
            .map_err(|e| LinkError::Network(e.to_string()))?;
        let result = resp.result(); // public accessor (field is pub(crate))
        let data = resp.into_response_data();
        let want_more = data.want_more();

        match finalize_step(attempt, MAX, result, want_more) {
            FinalizeStep::Done => return Ok(FinalizeResult::Done),
            FinalizeStep::WrongCode => return Ok(FinalizeResult::WrongCode),
            FinalizeStep::GiveUp => return Ok(FinalizeResult::TimeSyncFailed),
            FinalizeStep::Retry => continue,
        }
    }
    unreachable!("finalize loop always returns within MAX iterations")
}

/// Attach a new phone number to the account. Steam then emails a confirmation
/// link the user must click. `e164` must be like "+15551234567". Blocking.
// Intentionally collapse errors to `Ok(false)` — the wizard only needs success/failure here.
pub fn set_phone(tokens: &Tokens, proxy: Option<&Proxy>, e164: &str) -> Result<bool, LinkError> {
    use steamguard::phonelinker::{PhoneLinker, PhoneNumber};
    use steamguard::steamapi::PhoneClient;
    let phone: PhoneNumber = e164
        .parse()
        .map_err(|_| LinkError::Network("invalid phone number".into()))?;
    let linker = PhoneLinker::new(PhoneClient::new(transport(proxy)), tokens.clone());
    match linker.set_account_phone_number(phone) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Poll whether the account is still waiting for the emailed phone-confirmation
/// link to be clicked. `Some(seconds)` = still waiting; `None` = confirmed. Blocking.
pub fn await_phone_email(tokens: &Tokens, proxy: Option<&Proxy>) -> Result<Option<u32>, LinkError> {
    use steamguard::phonelinker::PhoneLinker;
    use steamguard::steamapi::PhoneClient;
    let linker = PhoneLinker::new(PhoneClient::new(transport(proxy)), tokens.clone());
    linker
        .is_account_waiting_for_email_confirmation()
        .map_err(|e| LinkError::Network(e.to_string()))
}

/// Ask Steam to send the SMS verification code to the attached phone. Blocking.
// Intentionally collapse errors to `Ok(false)` — the wizard only needs success/failure here.
pub fn send_sms(tokens: &Tokens, proxy: Option<&Proxy>) -> Result<bool, LinkError> {
    use steamguard::phonelinker::PhoneLinker;
    use steamguard::steamapi::PhoneClient;
    let linker = PhoneLinker::new(PhoneClient::new(transport(proxy)), tokens.clone());
    match linker.send_phone_verification_code(0) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// The single in-progress linking session, held in `AppState`.
/// Linking is modal (one at a time), so there is no session id.
#[derive(Default)]
pub struct LinkSession {
    pub proxy: Option<Proxy>,
    /// Kept alive only during the email-Guard-code pause (between begin and submit).
    pub login: Option<UserLogin<WebApiTransport>>,
    pub tokens: Option<Tokens>,
    pub steam_id: Option<u64>,
    /// Set after AddAuthenticator succeeds — carries the account + raw secret.
    pub linked: Option<LinkedData>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_type_maps_sms_and_email() {
        assert_eq!(ConfirmType::from_i32(1), ConfirmType::Sms);
        assert_eq!(ConfirmType::from_i32(2), ConfirmType::Sms);
        assert_eq!(ConfirmType::from_i32(3), ConfirmType::Email);
        assert_eq!(ConfirmType::from_i32(0), ConfirmType::Email); // unknown → email
        assert_eq!(ConfirmType::Sms.as_str(), "sms");
        assert_eq!(ConfirmType::Email.as_str(), "email");
    }

    #[test]
    fn needs_email_guard_detects_email_code() {
        let email = vec![EAuthSessionGuardType::k_EAuthSessionGuardType_EmailCode];
        let device = vec![EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceConfirmation];
        assert!(needs_email_guard(&email));
        assert!(!needs_email_guard(&device));
        assert!(!needs_email_guard(&[]));
    }

    use steamguard::protobufs::service_twofactor::CTwoFactor_AddAuthenticator_Response;
    use steamguard::steamapi::EResult;

    #[test]
    fn add_response_no_phone_maps_need_phone() {
        let mut resp = CTwoFactor_AddAuthenticator_Response::new();
        assert!(matches!(
            map_add_response(EResult::NoVerifiedPhone, &mut resp, "android:x", 1),
            AddOutcome::NeedPhone
        ));
        assert!(matches!(
            map_add_response(EResult::Fail, &mut resp, "android:x", 1),
            AddOutcome::NeedPhone
        ));
    }

    #[test]
    fn add_response_duplicate_maps_already_linked() {
        let mut resp = CTwoFactor_AddAuthenticator_Response::new();
        assert!(matches!(
            map_add_response(EResult::DuplicateRequest, &mut resp, "android:x", 1),
            AddOutcome::AlreadyLinked
        ));
    }

    #[test]
    fn add_response_ok_builds_account_and_confirm_type() {
        let mut resp = CTwoFactor_AddAuthenticator_Response::new();
        resp.set_account_name("alice".to_string());
        resp.set_revocation_code("R12345".to_string());
        resp.set_shared_secret(vec![1, 2, 3, 4]);
        resp.set_identity_secret(vec![5, 6, 7, 8]);
        resp.set_server_time(1_700_000_000);
        resp.set_confirm_type(3); // email
        resp.set_phone_number_hint(String::new());

        match map_add_response(EResult::OK, &mut resp, "android:dev", 76561199000000001) {
            AddOutcome::Code(d) => {
                assert_eq!(d.account.account_name, "alice");
                assert_eq!(d.account.revocation_code, "R12345");
                assert_eq!(d.account.steam_id, "76561199000000001");
                assert_eq!(d.account.device_id, "android:dev");
                assert_eq!(d.shared_secret_bytes, vec![1, 2, 3, 4]);
                assert_eq!(d.account.shared_secret, STANDARD.encode([1, 2, 3, 4]));
                assert_eq!(d.confirm_type, ConfirmType::Email);
                assert_eq!(d.server_time, 1_700_000_000);
            }
            other => panic!("expected Code, got {other:?}"),
        }
    }

    #[test]
    fn finalize_step_logic() {
        // OK + no want_more → done
        assert_eq!(finalize_step(0, 30, EResult::OK, false), FinalizeStep::Done);
        // OK + want_more, room left → retry; last attempt → give up
        assert_eq!(finalize_step(0, 30, EResult::OK, true), FinalizeStep::Retry);
        assert_eq!(finalize_step(29, 30, EResult::OK, true), FinalizeStep::GiveUp);
        // wrong confirmation code → WrongCode
        assert_eq!(
            finalize_step(0, 30, EResult::TwoFactorActivationCodeMismatch, false),
            FinalizeStep::WrongCode
        );
        // our TOTP rejected → retry until cap
        assert_eq!(finalize_step(0, 30, EResult::TwoFactorCodeMismatch, false), FinalizeStep::Retry);
        assert_eq!(finalize_step(29, 30, EResult::TwoFactorCodeMismatch, false), FinalizeStep::GiveUp);
    }
}
