//! AWS Cognito public-client adapter for the native authentication boundary.

use std::{collections::HashMap, fmt, time::Duration};

use async_trait::async_trait;
use aws_sdk_cognitoidentityprovider::{
    Client,
    config::{BehaviorVersion, Config, Region, retry::RetryConfig, timeout::TimeoutConfig},
    error::SdkError,
    operation::{
        confirm_forgot_password::ConfirmForgotPasswordError,
        forgot_password::ForgotPasswordError,
        initiate_auth::{InitiateAuthError, InitiateAuthOutput},
        respond_to_auth_challenge::{RespondToAuthChallengeError, RespondToAuthChallengeOutput},
    },
    types::{AuthFlowType, AuthenticationResultType, ChallengeNameType},
};
use zeroize::{Zeroize, Zeroizing};

use super::{
    CognitoAuthStep, CognitoChallengeKind, CognitoChallengeStep, CognitoProvider, CognitoRefresh,
    CognitoTokenSet, NativeAuthError, NativeAuthErrorCode,
};

const MAX_CLIENT_ID_BYTES: usize = 128;
const MAX_REGION_BYTES: usize = 64;

/// Cognito SDK provider configured without an application-client secret or browser OAuth surface.
pub struct AwsCognitoProvider {
    client: Client,
    client_id: String,
}

impl AwsCognitoProvider {
    /// Builds the public Cognito client with fixed native operation timeouts.
    pub fn new(region: &str, client_id: &str) -> Result<Self, NativeAuthError> {
        validate_public_configuration(region, client_id)?;
        let config = provider_config(region);
        Ok(Self {
            client: Client::from_conf(config),
            client_id: client_id.to_owned(),
        })
    }

    #[cfg(test)]
    fn from_client(client: Client, client_id: &str) -> Result<Self, NativeAuthError> {
        validate_public_configuration("us-east-1", client_id)?;
        Ok(Self {
            client,
            client_id: client_id.to_owned(),
        })
    }
}

fn provider_config(region: &str) -> Config {
    let timeouts = TimeoutConfig::builder()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(10))
        .operation_attempt_timeout(Duration::from_secs(12))
        .operation_timeout(Duration::from_secs(15))
        .build();
    Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(region.to_owned()))
        // Authentication requests are deliberately single-attempt. In particular,
        // retrying NEW_PASSWORD_REQUIRED after a lost response could repeat a
        // state-changing submission whose first result is unknowable.
        .retry_config(RetryConfig::disabled())
        .timeout_config(timeouts)
        .build()
}

impl fmt::Debug for AwsCognitoProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AwsCognitoProvider")
            .field("configuration", &"[configured]")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CognitoProvider for AwsCognitoProvider {
    async fn initiate_srp(
        &self,
        parameters: HashMap<String, String>,
    ) -> Result<CognitoAuthStep, NativeAuthError> {
        // The SDK request builder requires owned strings. These buffers are the
        // only unavoidable SDK-owned credential copies and live only until send returns.
        let output = self
            .client
            .initiate_auth()
            .auth_flow(AuthFlowType::UserSrpAuth)
            .client_id(&self.client_id)
            .set_auth_parameters(Some(parameters))
            .send()
            .await
            .map_err(map_initiate_error)?;
        parse_initiate_output(output)
    }

    async fn respond(
        &self,
        kind: CognitoChallengeKind,
        parameters: HashMap<String, String>,
        session: &str,
    ) -> Result<CognitoAuthStep, NativeAuthError> {
        let challenge_name = provider_challenge(kind)?;
        // Cognito's SDK request owns the response parameters and opaque session
        // until this single network attempt finishes; the adapter retains neither.
        let output = self
            .client
            .respond_to_auth_challenge()
            .challenge_name(challenge_name)
            .client_id(&self.client_id)
            .session(session)
            .set_challenge_responses(Some(parameters))
            .send()
            .await
            .map_err(|error| map_respond_error(kind, error))?;
        let step = parse_respond_output(output);
        if kind == CognitoChallengeKind::NewPasswordRequired {
            step.map_err(|_| invitation_completion_uncertain())
        } else {
            step
        }
    }

    async fn refresh(
        &self,
        refresh_material: &crate::credential_store::SecretBytes,
    ) -> Result<CognitoRefresh, NativeAuthError> {
        let refresh_material = String::from_utf8(refresh_material.as_bytes().to_vec())
            .map(Zeroizing::new)
            .map_err(|_| NativeAuthError::new(NativeAuthErrorCode::InvalidCredentials))?;
        let output = self
            .client
            .initiate_auth()
            .auth_flow(AuthFlowType::RefreshTokenAuth)
            .client_id(&self.client_id)
            .auth_parameters("REFRESH_TOKEN", refresh_material.as_str())
            .send()
            .await
            .map_err(map_initiate_error)?;
        parse_refresh_output(output)
    }

    async fn begin_password_reset(&self, identifier: &str) -> Result<(), NativeAuthError> {
        match self
            .client
            .forgot_password()
            .client_id(&self.client_id)
            .username(identifier)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(error) => map_forgot_password_error(error),
        }
    }

    async fn confirm_password_reset(
        &self,
        identifier: &str,
        code: &str,
        new_password: &str,
    ) -> Result<(), NativeAuthError> {
        self.client
            .confirm_forgot_password()
            .client_id(&self.client_id)
            .username(identifier)
            .confirmation_code(code)
            .password(new_password)
            .send()
            .await
            .map(|_| ())
            .map_err(map_confirm_password_reset_error)
    }
}

fn parse_initiate_output(output: InitiateAuthOutput) -> Result<CognitoAuthStep, NativeAuthError> {
    parse_output(
        output.challenge_name,
        output.challenge_parameters,
        output.session,
        output.authentication_result,
    )
}

fn parse_respond_output(
    output: RespondToAuthChallengeOutput,
) -> Result<CognitoAuthStep, NativeAuthError> {
    parse_output(
        output.challenge_name,
        output.challenge_parameters,
        output.session,
        output.authentication_result,
    )
}

fn parse_refresh_output(output: InitiateAuthOutput) -> Result<CognitoRefresh, NativeAuthError> {
    if output.challenge_name.is_some()
        || output.challenge_parameters.is_some()
        || output.session.is_some()
    {
        return Err(invalid_response());
    }
    let mut result =
        SensitiveAuthenticationResult(output.authentication_result.ok_or_else(invalid_response)?);
    if result.0.token_type() != Some("Bearer")
        || result.0.new_device_metadata.is_some()
        || result.0.access_token.is_none()
        || result.0.refresh_token.is_some()
    {
        return Err(invalid_response());
    }
    let valid_for = u64::try_from(result.0.expires_in())
        .ok()
        .map(Duration::from_secs)
        .ok_or_else(invalid_response)?;
    CognitoRefresh::new(
        result.0.access_token.take().ok_or_else(invalid_response)?,
        valid_for,
    )
}

fn parse_output(
    challenge_name: Option<ChallengeNameType>,
    mut parameters: Option<HashMap<String, String>>,
    mut session: Option<String>,
    authentication_result: Option<AuthenticationResultType>,
) -> Result<CognitoAuthStep, NativeAuthError> {
    match (challenge_name, authentication_result) {
        (Some(_), Some(result)) => {
            let _result = SensitiveAuthenticationResult(result);
            zeroize_response_side_data(&mut parameters, &mut session);
            Err(invalid_response())
        }
        (None, None) => {
            zeroize_response_side_data(&mut parameters, &mut session);
            Err(invalid_response())
        }
        (Some(challenge_name), None) => {
            let kind = native_challenge(&challenge_name);
            let challenge = CognitoChallengeStep::new(
                kind,
                parameters.take().unwrap_or_default(),
                session.take().unwrap_or_default(),
            )?;
            Ok(CognitoAuthStep::Challenge(challenge))
        }
        (None, Some(result)) => {
            // The SDK necessarily owns token strings while decoding its response.
            // This guard scrubs every token buffer not moved into a zeroizing native value.
            let mut result = SensitiveAuthenticationResult(result);
            if parameters.is_some()
                || session.is_some()
                || result.0.token_type() != Some("Bearer")
                || result.0.new_device_metadata.is_some()
                || result.0.access_token.is_none()
                || result.0.refresh_token.is_none()
            {
                zeroize_response_side_data(&mut parameters, &mut session);
                return Err(invalid_response());
            }
            let valid_for = u64::try_from(result.0.expires_in())
                .ok()
                .map(Duration::from_secs)
                .ok_or_else(invalid_response)?;
            Ok(CognitoAuthStep::Authenticated(CognitoTokenSet::new(
                result.0.access_token.take().ok_or_else(invalid_response)?,
                result.0.refresh_token.take().ok_or_else(invalid_response)?,
                valid_for,
            )?))
        }
    }
}

struct SensitiveAuthenticationResult(AuthenticationResultType);

impl Drop for SensitiveAuthenticationResult {
    fn drop(&mut self) {
        for token in [
            &mut self.0.access_token,
            &mut self.0.refresh_token,
            &mut self.0.id_token,
        ] {
            if let Some(token) = token.as_mut() {
                token.zeroize();
            }
        }
        if let Some(device) = self.0.new_device_metadata.as_mut() {
            if let Some(device_key) = device.device_key.as_mut() {
                device_key.zeroize();
            }
            if let Some(device_group_key) = device.device_group_key.as_mut() {
                device_group_key.zeroize();
            }
        }
    }
}

fn zeroize_response_side_data(
    parameters: &mut Option<HashMap<String, String>>,
    session: &mut Option<String>,
) {
    if let Some(parameters) = parameters.as_mut() {
        for value in parameters.values_mut() {
            value.zeroize();
        }
    }
    if let Some(session) = session.as_mut() {
        session.zeroize();
    }
}

fn native_challenge(challenge: &ChallengeNameType) -> CognitoChallengeKind {
    match challenge {
        ChallengeNameType::PasswordVerifier => CognitoChallengeKind::PasswordVerifier,
        ChallengeNameType::NewPasswordRequired => CognitoChallengeKind::NewPasswordRequired,
        ChallengeNameType::SoftwareTokenMfa => CognitoChallengeKind::SoftwareTokenMfa,
        ChallengeNameType::MfaSetup => CognitoChallengeKind::MfaSetup,
        ChallengeNameType::EmailOtp => CognitoChallengeKind::EmailCode,
        _ => CognitoChallengeKind::Unsupported,
    }
}

fn provider_challenge(kind: CognitoChallengeKind) -> Result<ChallengeNameType, NativeAuthError> {
    match kind {
        CognitoChallengeKind::PasswordVerifier => Ok(ChallengeNameType::PasswordVerifier),
        CognitoChallengeKind::NewPasswordRequired => Ok(ChallengeNameType::NewPasswordRequired),
        CognitoChallengeKind::SoftwareTokenMfa => Ok(ChallengeNameType::SoftwareTokenMfa),
        CognitoChallengeKind::MfaSetup => Ok(ChallengeNameType::MfaSetup),
        CognitoChallengeKind::EmailCode => Ok(ChallengeNameType::EmailOtp),
        CognitoChallengeKind::Unsupported => Err(invalid_response()),
    }
}

fn map_initiate_error(error: SdkError<InitiateAuthError>) -> NativeAuthError {
    let Some(error) = error.as_service_error() else {
        return NativeAuthError::new(NativeAuthErrorCode::Unavailable);
    };
    map_initiate_service_error(error)
}

fn map_initiate_service_error(error: &InitiateAuthError) -> NativeAuthError {
    if error.is_too_many_requests_exception() {
        NativeAuthError::new(NativeAuthErrorCode::RateLimited)
    } else if error.is_not_authorized_exception()
        || error.is_user_not_found_exception()
        || error.is_user_not_confirmed_exception()
        || error.is_password_reset_required_exception()
    {
        NativeAuthError::new(NativeAuthErrorCode::InvalidCredentials)
    } else if error.is_invalid_parameter_exception() {
        invalid_response()
    } else {
        NativeAuthError::new(NativeAuthErrorCode::Unavailable)
    }
}

fn map_respond_error(
    kind: CognitoChallengeKind,
    error: SdkError<RespondToAuthChallengeError>,
) -> NativeAuthError {
    let Some(error) = error.as_service_error() else {
        return if kind == CognitoChallengeKind::NewPasswordRequired {
            invitation_completion_uncertain()
        } else {
            NativeAuthError::new(NativeAuthErrorCode::Unavailable)
        };
    };
    map_respond_service_error(error)
}

fn map_forgot_password_error(error: SdkError<ForgotPasswordError>) -> Result<(), NativeAuthError> {
    let Some(error) = error.as_service_error() else {
        return Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable));
    };
    if error.is_too_many_requests_exception() {
        Err(NativeAuthError::new(NativeAuthErrorCode::RateLimited))
    } else if error.is_not_authorized_exception() || error.is_user_not_found_exception() {
        // Preserve an indistinguishable recovery response so this public client
        // cannot become an account-existence oracle.
        Ok(())
    } else {
        Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable))
    }
}

fn map_confirm_password_reset_error(
    error: SdkError<ConfirmForgotPasswordError>,
) -> NativeAuthError {
    let Some(error) = error.as_service_error() else {
        return NativeAuthError::new(NativeAuthErrorCode::Unavailable);
    };
    if error.is_too_many_requests_exception() {
        NativeAuthError::new(NativeAuthErrorCode::RateLimited)
    } else if error.is_invalid_password_exception() {
        NativeAuthError::new(NativeAuthErrorCode::PasswordRejected)
    } else if error.is_code_mismatch_exception()
        || error.is_expired_code_exception()
        || error.is_not_authorized_exception()
        || error.is_user_not_found_exception()
    {
        NativeAuthError::new(NativeAuthErrorCode::InvalidCredentials)
    } else {
        NativeAuthError::new(NativeAuthErrorCode::Unavailable)
    }
}

fn map_respond_service_error(error: &RespondToAuthChallengeError) -> NativeAuthError {
    if error.is_too_many_requests_exception() {
        NativeAuthError::new(NativeAuthErrorCode::RateLimited)
    } else if error.is_invalid_password_exception()
        || error.is_password_history_policy_violation_exception()
    {
        NativeAuthError::new(NativeAuthErrorCode::PasswordRejected)
    } else if error.is_not_authorized_exception()
        || error.is_user_not_found_exception()
        || error.is_user_not_confirmed_exception()
        || error.is_password_reset_required_exception()
        || error.is_expired_code_exception()
        || error.is_code_mismatch_exception()
    {
        NativeAuthError::new(NativeAuthErrorCode::InvalidCredentials)
    } else if error.is_invalid_parameter_exception() {
        invalid_response()
    } else {
        NativeAuthError::new(NativeAuthErrorCode::Unavailable)
    }
}

const fn invitation_completion_uncertain() -> NativeAuthError {
    NativeAuthError::new(NativeAuthErrorCode::InvitationCompletionUncertain)
}

fn validate_public_configuration(region: &str, client_id: &str) -> Result<(), NativeAuthError> {
    let valid_region = !region.is_empty()
        && region.len() <= MAX_REGION_BYTES
        && region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let valid_client = !client_id.is_empty()
        && client_id.len() <= MAX_CLIENT_ID_BYTES
        && client_id.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if valid_region && valid_client {
        Ok(())
    } else {
        Err(NativeAuthError::new(NativeAuthErrorCode::Unavailable))
    }
}

const fn invalid_response() -> NativeAuthError {
    NativeAuthError::new(NativeAuthErrorCode::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use aws_sdk_cognitoidentityprovider::{
        Client,
        config::{BehaviorVersion, Config, Region},
        error::SdkError,
        operation::{
            initiate_auth::{InitiateAuthError, InitiateAuthOutput},
            respond_to_auth_challenge::{
                RespondToAuthChallengeError, RespondToAuthChallengeOutput,
            },
        },
        types::{
            AuthenticationResultType, ChallengeNameType,
            error::{InvalidPasswordException, TooManyRequestsException},
        },
    };

    use super::{
        AwsCognitoProvider, CognitoAuthStep, CognitoChallengeKind, CognitoProvider,
        NativeAuthErrorCode, map_initiate_error, map_initiate_service_error, map_respond_error,
        map_respond_service_error, parse_initiate_output, parse_respond_output, provider_challenge,
        provider_config, validate_public_configuration,
    };

    fn token_result() -> AuthenticationResultType {
        AuthenticationResultType::builder()
            .access_token("access-token")
            .refresh_token("refresh-material")
            .expires_in(600)
            .token_type("Bearer")
            .build()
    }

    #[test]
    fn parses_tokens_without_accepting_id_tokens_for_authorization() {
        let output = InitiateAuthOutput::builder()
            .authentication_result(
                AuthenticationResultType::builder()
                    .access_token("access-token")
                    .refresh_token("refresh-material")
                    .id_token("never-authorize-with-this")
                    .expires_in(600)
                    .token_type("Bearer")
                    .build(),
            )
            .build();
        let CognitoAuthStep::Authenticated(tokens) = parse_initiate_output(output).unwrap() else {
            panic!("expected tokens");
        };
        assert_eq!(tokens.access_token(), "access-token");
        assert_eq!(tokens.valid_for(), Duration::from_secs(600));
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("access-token"));
        assert!(!debug.contains("never-authorize-with-this"));
    }

    #[test]
    fn parses_only_supported_bounded_challenges() {
        let output = RespondToAuthChallengeOutput::builder()
            .challenge_name(ChallengeNameType::SoftwareTokenMfa)
            .session("opaque-session")
            .set_challenge_parameters(Some(HashMap::new()))
            .build();
        let CognitoAuthStep::Challenge(challenge) = parse_respond_output(output).unwrap() else {
            panic!("expected challenge");
        };
        assert_eq!(challenge.kind(), CognitoChallengeKind::SoftwareTokenMfa);
        assert!(!format!("{challenge:?}").contains("opaque-session"));

        let unsupported = InitiateAuthOutput::builder()
            .challenge_name(ChallengeNameType::WebAuthn)
            .session("opaque-session")
            .build();
        assert_eq!(
            parse_initiate_output(unsupported).unwrap_err().code(),
            NativeAuthErrorCode::InvalidResponse
        );
    }

    #[test]
    fn contradictory_or_incomplete_provider_outputs_fail_closed() {
        for output in [
            InitiateAuthOutput::builder().build(),
            InitiateAuthOutput::builder()
                .challenge_name(ChallengeNameType::PasswordVerifier)
                .build(),
            InitiateAuthOutput::builder()
                .challenge_name(ChallengeNameType::PasswordVerifier)
                .session("opaque-session")
                .authentication_result(token_result())
                .build(),
            InitiateAuthOutput::builder()
                .authentication_result(
                    AuthenticationResultType::builder()
                        .access_token("access-token")
                        .refresh_token("refresh-material")
                        .expires_in(0)
                        .token_type("Bearer")
                        .build(),
                )
                .build(),
        ] {
            assert_eq!(
                parse_initiate_output(output).unwrap_err().code(),
                NativeAuthErrorCode::InvalidResponse
            );
        }
    }

    #[test]
    fn provider_configuration_and_challenge_mapping_are_allowlisted() {
        assert!(validate_public_configuration("us-east-1", "client123").is_ok());
        assert!(validate_public_configuration("", "client123").is_err());
        assert!(validate_public_configuration("us-east-1", "bad_client").is_err());
        assert_eq!(
            provider_challenge(CognitoChallengeKind::NewPasswordRequired).unwrap(),
            ChallengeNameType::NewPasswordRequired
        );
        assert!(provider_challenge(CognitoChallengeKind::Unsupported).is_err());

        assert!(AwsCognitoProvider::new("us-east-1", "client123").is_ok());
        assert_eq!(
            provider_config("us-east-1")
                .retry_config()
                .unwrap()
                .max_attempts(),
            1
        );
        assert_eq!(
            map_initiate_error(SdkError::<InitiateAuthError>::construction_failure(
                std::io::Error::other("connection failed"),
            ))
            .code(),
            NativeAuthErrorCode::Unavailable
        );
        assert_eq!(
            map_respond_error(
                CognitoChallengeKind::PasswordVerifier,
                SdkError::<RespondToAuthChallengeError>::construction_failure(
                    std::io::Error::other("connection failed"),
                )
            )
            .code(),
            NativeAuthErrorCode::Unavailable
        );
        assert_eq!(
            map_respond_error(
                CognitoChallengeKind::NewPasswordRequired,
                SdkError::<RespondToAuthChallengeError>::construction_failure(
                    std::io::Error::other("response lost"),
                )
            )
            .code(),
            NativeAuthErrorCode::InvitationCompletionUncertain
        );

        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .build();
        let provider =
            AwsCognitoProvider::from_client(Client::from_conf(config), "client123").unwrap();
        assert!(!format!("{provider:?}").contains("client123"));
    }

    #[test]
    fn service_error_fixtures_map_without_exposing_upstream_messages() {
        let throttled = InitiateAuthError::TooManyRequestsException(
            TooManyRequestsException::builder()
                .message("identifier-specific upstream detail")
                .build(),
        );
        let throttled = map_initiate_service_error(&throttled);
        assert_eq!(throttled.code(), NativeAuthErrorCode::RateLimited);
        assert!(!throttled.message().contains("identifier-specific"));

        let rejected = RespondToAuthChallengeError::InvalidPasswordException(
            InvalidPasswordException::builder()
                .message("password-policy upstream detail")
                .build(),
        );
        let rejected = map_respond_service_error(&rejected);
        assert_eq!(rejected.code(), NativeAuthErrorCode::PasswordRejected);
        assert!(!rejected.message().contains("upstream detail"));
    }

    #[test]
    fn provider_rejects_invalid_native_challenges_and_refresh_material_before_network_use() {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .build();
        let provider =
            AwsCognitoProvider::from_client(Client::from_conf(config), "client123").unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(provider.respond(
                CognitoChallengeKind::Unsupported,
                HashMap::new(),
                "opaque-session",
            ))
            .unwrap_err()
            .code(),
            NativeAuthErrorCode::InvalidResponse
        );
        assert_eq!(
            tauri::async_runtime::block_on(
                provider.refresh(&crate::credential_store::SecretBytes::new(vec![0xff],))
            )
            .unwrap_err()
            .code(),
            NativeAuthErrorCode::InvalidCredentials
        );
    }

    #[test]
    fn provider_maps_a_local_unavailable_endpoint_without_exposing_request_values() {
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .endpoint_url("http://127.0.0.1:9")
            .build();
        let provider =
            AwsCognitoProvider::from_client(Client::from_conf(config), "client123").unwrap();
        for result in [
            tauri::async_runtime::block_on(provider.initiate_srp(HashMap::new())),
            tauri::async_runtime::block_on(provider.respond(
                CognitoChallengeKind::PasswordVerifier,
                HashMap::new(),
                "opaque-session",
            )),
        ] {
            assert_eq!(result.unwrap_err().code(), NativeAuthErrorCode::Unavailable);
        }
        assert_eq!(
            tauri::async_runtime::block_on(provider.refresh(
                &crate::credential_store::SecretBytes::new(b"opaque-refresh-material".to_vec(),)
            ))
            .unwrap_err()
            .code(),
            NativeAuthErrorCode::Unavailable
        );
    }
}
