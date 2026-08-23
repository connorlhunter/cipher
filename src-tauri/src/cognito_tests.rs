use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    time::Duration,
};

use serde_json::json;

use super::*;

const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_Cipher";
const CLIENT_ID: &str = "cipher-public-client";
const SUBJECT: &str = "7f2bd9a1-6b88-4f86-b741-5c68dbfdf570";
const NOW: i64 = 1_800_000_000;
const TOKEN_EXPIRATION: i64 = 2_000_000_000;
const ALPHA_MODULUS: &str = "wMb9CptELdqI2cBgJWhXIxVRDEIyk262p2u_4CijArBHvg70RJcEmv5nIdqOCY_lmIp3D0WI0syRkoeYvH2ypDJJrYLi9birzR39vn5sLfkg1WW363PO6lVE9Y92JXR0DH8RFaN0xHTroKxvZU1qllHoUfJj8m9Xr2Lnji1xVIL1RTJj_034fHyFztaUazxpNf4dipTOCw--psFrH3deQdvW0nrSfWx92Cd75qTEKYb1y-N1Hxp5UGrKa6v1Z4UaKke0Jd6qvz3KxzrpZ059WoJGaG0dfFT2WpYJ9k8lv75CXH8WotM4owszCpBEhbrCbOp9dmKWbJaLJAv4IZZsuQ";
const BETA_MODULUS: &str = "6FRY8eDZj_hos_oePVNKa5lIYHFUpqenBr_LNUi-UfbvpHANWq68-R3SmpkYMVbuyDDXFlnbQA7VjyQE1Br-TDtHOCzGP-UHLQuo_GAVevf743d3b2NUYDf3O8gekE4IZh4QrqZM5NwSKdkyatqVqvAYrsbaWraprv4KJALzFRmEwOxVtft-Ixf6F2i0OVD6TKTLCF5UyhgV0DX_dpogGADqjE1IKNU3t7qvf33dh5lEKMEbMqntLpQvxWjkHAryn_-a-LWH5dfcClA5y26HaXn4Tfac9eFDdNaWEgGN_2WmM9jyvw9UF5xD1ry3zjhpTV_OMZrpNOmfoCs9Xw37wQ";
const RSA_EXPONENT: &str = "AQAB";

// These are non-secret signed fixtures; their private keys are not stored in the repository.
const VALID_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiI3ZjJiZDlhMS02Yjg4LTRmODYtYjc0MS01YzY4ZGJmZGY1NzAifQ.YYfEoIBHuaEzWrPAwY-EGafkhrORZ1JMC37bURn_VjAjEHlYkn4EdZyd4j09llAYLsCrR-f-7hs1RVn4GSWONlmbatcsL9Q1ol1TJEN9BseFy1GXOHRGW85pnE26FsQaKX0Uqs7aBWXub4mtt1bQnkwgRMQ8G0z87UndTlGCntVrdZnsjRTj79bF7MKql4Vh8sE2yWmOTYafH_Abco57QBg0usf2dWkBoTHolFm5PTTDMBvZuQ1fkkgWgfYooRroqobxGx-RCEtFH3sTWkpfVBdJQGbLIHvjX20fYhQIpueQ0jrhjUNugRPEnzSPe6-bPyt-_i-GUnfiUV5Z0I6Q-Q";
const WRONG_ISSUER_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLXdlc3QtMi5hbWF6b25hd3MuY29tL3VzLXdlc3QtMl9Xcm9uZyIsImNsaWVudF9pZCI6ImNpcGhlci1wdWJsaWMtY2xpZW50IiwidG9rZW5fdXNlIjoiYWNjZXNzIiwiZXhwIjoyMDAwMDAwMDAwLCJzY29wZSI6ImNpcGhlcjpyZWFkIGNpcGhlcjp3cml0ZSIsInN1YiI6IjdmMmJkOWExLTZiODgtNGY4Ni1iNzQxLTVjNjhkYmZkZjU3MCJ9.qjwEVJAiPZ_AUr_oaJUppeGNAaNTDrMw_cjGAI8fsaJQ07Ucrw13MmXy1RS6DtEYD-xOSAWjybnqLg_ZdPJdrHMiLRSwHPayGFBRAwauIfKAM3Q2p6_bROu2yf48O-y5wKUPQunK2F5C7nw7c4O7Wj01AsQQ4xFpQBmD3eCJ8j4M9C31vFwtgcyOHl3CNQcqV-6LUhURi4xTbXaMdZ96MB7xrdZhjIcbpe0AZoNYCwmNQHADaMnD7O-_0iBEMGE65RrccaqcI453tWkZSNjgBuVm-AMLMKSdc1wjJoc1nxVMgxKz7vOKnwKOjc7_eywjHf2hMzko8lbq7Lul7V5tgg";
const WRONG_CLIENT_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJvdGhlci1jbGllbnQiLCJ0b2tlbl91c2UiOiJhY2Nlc3MiLCJleHAiOjIwMDAwMDAwMDAsInNjb3BlIjoiY2lwaGVyOnJlYWQgY2lwaGVyOndyaXRlIiwic3ViIjoiN2YyYmQ5YTEtNmI4OC00Zjg2LWI3NDEtNWM2OGRiZmRmNTcwIn0.agrTiJfCfE5bIodBJh_JGtSqenmrPm8ZPFLVV5LNACwraN65yHw6pp9dBCbznlnD_nHxU29GMBbp5W-xHbmWt_7F1Nw1WQiOEnp9tcUgsoZBAK3ppEtvcBaXexvRSSo1IVlYtrymts2zsDNw5_nZtNxj8W2zo862xNX5VKIcJ8PaWluBb7aUu2AGXEwzoDgtz-cGHXJPOiztLO9pSO6CspS4c38mEggqNH-ilu5vUgBNmmjlosccKxxyG889wMBnSy4U5dnF0RCU4seawS4gSXPoqD-zgDMqbyscFypt2W-3QcGrmz0lhJQYds0aunwzPIYWCm5P-RTza6uZ2aWXnQ";
const WRONG_TOKEN_USE_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImlkIiwiZXhwIjoyMDAwMDAwMDAwLCJzY29wZSI6ImNpcGhlcjpyZWFkIGNpcGhlcjp3cml0ZSIsInN1YiI6IjdmMmJkOWExLTZiODgtNGY4Ni1iNzQxLTVjNjhkYmZkZjU3MCJ9.RH4QUKMf2ESlI73VyJwfXi_cTpFNLSE1MCEQcp-G5mXo-l7LiDPL5wc7rjNo-gKGOI-rdJJ4uvSo2-nNrs-8XQTaEyDKxF5vpo6Tw-IRtzUz_cenVRKoU5arDPTtFhOs4L0L4P9UEFiYHHslO9DJPczhgh-7RWC06IGcaL0Y_5ckeSXtyvnt_h1bvi582RKT-0l_HFT6w6UZrTSiILiNLQotG2tMmE1v4wLbE8vHJ3jhgFbsnJnXAWRcZEROiJpx05oxGRU-90OQTngpQLcd7rDKpHVcZhr9dK7gN0z0chUtRdCYpsLKvfFkGbCIM09hkfR1F3EotPZpuhrBOY3JXw";
const EXPIRED_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MTcwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiI3ZjJiZDlhMS02Yjg4LTRmODYtYjc0MS01YzY4ZGJmZGY1NzAifQ.NLhusma4lYBG7OPFExPj8COLzDQkBBEw_ROooU8AzbwZApiKpln2QLHPuBmMzUk6ZCveT6jL1vP2PlDyd-r7OMqSvgoRWWMlC2ys6xepHko-A-k6VESx_ogamORHFRoBbaLw6BKvbLy2yWTEC7AYCvG1u5SuLcxREZAu0mOa7659fBqJcVIRGlsB0IoOlbULOeM1fVC1-1C8PTv49Behf-zzcCXxGG2A84GR8AaGLhBNzU8nnJTSSyBZ5BViaU1VVd8gTssnS5-NZQyDGWWvXJ8JlYY0_VQSqkqExWe3KaRs7IwkJNai0PqEwzPnA53KKsu1DSTZ5ivTLaCoO6DwXw";
const MISSING_SCOPE_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCIsInN1YiI6IjdmMmJkOWExLTZiODgtNGY4Ni1iNzQxLTVjNjhkYmZkZjU3MCJ9.UX-7gExSfkSJ3G3mKyw0GtgKSW6wYQzW-xFayZ251iu30er9X3unZRdUOJRVyTYAPNmcihHsCcpnJtjsfcEwwMkXoWTnkD5rH0AjvU6lCjB9uR9SplR_zOPMKU9Qd5ZBtqmUc6U1XzoHFmo-c9KKd0oEliGxNkeoV_pZrV2yMltcu4f8RjBqNIV5o_ipE-HS0MkZ9D9tljKGipHQg3MQduI55MSoAqBtugORKNl2y5eFbx9ttz6Em-RYOiq186N1jSHN1TQmnTB6XH2HAl0mjXT8gjxlrotOEs4sOVIy2yAOMbyh6Ul3bLpFUJcHqR9L-kmy19n-WWMauVetMarvrg";
const DUPLICATE_SCOPE_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiI3ZjJiZDlhMS02Yjg4LTRmODYtYjc0MS01YzY4ZGJmZGY1NzAifQ.F-MJRUWRoDIbU18SAKrkwV_e7uAH0VyqPcIDJZx-CkWJ1OlAvIwAhiZWQKZa6EWRcOuelTKiBahaaSKLUDIMpB02fPdjfJzYjfmDc65d3fQ-G2DNHnSIWj2z70ECwS735xyjq6ped4gIE-3eiyoBZs7pbj7JzHKEChGiLrhcEnsP2784kSq87DxLdxZ1KA8dn5ceUqCSzekzuUjJYzydpidSPvAA4ExU7cBEqnyWQ5rkrPmOUfhQuNDOI9XcOOuMeKrPl4E1aImLVB8aZSKK9ew_rxTgrauuz3Ul2zMn5dYWW_2a6NMJBBsPDtp1fN54BbtjXM-W0UImHFQaQgxVMQ";
const INVALID_SUBJECT_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiJpbnZhbGlkIHN1YmplY3QifQ.af-yJ_dUAiNZ2Aa6uLhwnJSgqNjwd7iFF1ZX96KUdSICL-K_7AMS6tHADmLSDQn9NRHe1WcaHUA5UaqnvXqbJFBeSRZYC_ip77RwXajtSDi8EnmIeH4M6ih478IEo0AfONxkWb1F6fbpmzkp8p3KE0weGxVFvQLKFa1X7Hx3iMMa9NmUBIVzBUpoEEk-FBRezL803NHV5yOkQjcnL9ut744S5dFAI7vAuiK1Z18DmMKvoWxZSRon-ptmjcuF0-EThmCK-RgQvbL17U5cj71MBk5DORlrsXK3eR2Wi-XYsMieSPqaDEOjSNwoREa6nBYQSGSuftN5SvGcbG_ELOiZOg";
const BETA_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImJldGEiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiI3ZjJiZDlhMS02Yjg4LTRmODYtYjc0MS01YzY4ZGJmZGY1NzAifQ.QY-S0PssoPY5qjfydhsqGJfW7wnIOp3259CwGO8CX3qreHWi6sPs_aV1dZOeEcangxtTsU8zsLhbDAn2jdKcZ7WqJ7nZzFX_UigVqmoFye22_4dKU7XakCVYAHtEtmCX9kOe5Zn_V-rlHV9jVrAF_wjNCcHY5urIADe4crNYfrSQZwfpLbN4niAAWPLCTf8Q8lGyTfPgKIAND1WRz_qPqHB-CxK-e9Lu1ZFIoG5vH5trZBolUE5Nga11SRYL5J4-NN7KKgkS6AoKnI0QfEU5T6V_d2b-ROCCCXVxygl0jgGI0XFlrngV8PYvJST3V8YqgN-pNBRM-IEYW_usJzk7mQ";
const ROTATED_ALPHA_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2NvZ25pdG8taWRwLnVzLWVhc3QtMS5hbWF6b25hd3MuY29tL3VzLWVhc3QtMV9DaXBoZXIiLCJjbGllbnRfaWQiOiJjaXBoZXItcHVibGljLWNsaWVudCIsInRva2VuX3VzZSI6ImFjY2VzcyIsImV4cCI6MjAwMDAwMDAwMCwic2NvcGUiOiJjaXBoZXI6cmVhZCBjaXBoZXI6d3JpdGUiLCJzdWIiOiI3ZjJiZDlhMS02Yjg4LTRmODYtYjc0MS01YzY4ZGJmZGY1NzAifQ.WFuipRGNIQgrv5z9yL3x0ySEyDqsJiIKoLovqIU0Vo9NmD-xGZvfU7nRtXuelZJQ9RplWu42yiOGBl3WDEUUsk5n7JnXpbWjKfzr7VhZgUDdsZnavyB5eRiqQJUeBryF2ZueYyyrzdn7wkGuqFfoW-0ppH5TgUWXrkvKZDQ93n1SezKPM4L3kY1adgKlRIFraWoe1yt3k-rImbv_CS0ZE-jVkr275t9xdRk0CunsCC9f4g4se7iaANm5V9b4MRzChbBtC49HmlGUgMck-pZs-qo0LGhi9StFVnjoC-5FEl4rXmLPpvxjHpqZOb48VPlrmsxOaaXKe27tKg4GABAz3Q";

struct SequenceJwksSource {
    responses: RefCell<VecDeque<Result<String, JwksSourceError>>>,
    calls: Cell<usize>,
}

impl SequenceJwksSource {
    fn new(responses: impl IntoIterator<Item = Result<String, JwksSourceError>>) -> Self {
        Self {
            responses: RefCell::new(responses.into_iter().collect()),
            calls: Cell::new(0),
        }
    }
}

impl JwksSource for SequenceJwksSource {
    fn fetch_jwks(&self) -> Result<String, JwksSourceError> {
        self.calls.set(self.calls.get() + 1);
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(JwksSourceError::Unavailable))
    }
}

fn policy(max_age: Duration) -> CognitoTokenPolicy {
    CognitoTokenPolicy::new(ISSUER, CLIENT_ID, ["cipher:read", "cipher:write"], max_age).unwrap()
}

fn jwks(keys: &[(&str, &str)]) -> String {
    json!({
        "keys": keys
            .iter()
            .map(|(kid, modulus)| json!({
                "kid": kid,
                "kty": "RSA",
                "alg": "RS256",
                "use": "sig",
                "n": modulus,
                "e": RSA_EXPONENT,
            }))
            .collect::<Vec<_>>()
    })
    .to_string()
}

fn alpha_jwks() -> String {
    jwks(&[("alpha", ALPHA_MODULUS)])
}

fn validator_with(
    responses: impl IntoIterator<Item = Result<String, JwksSourceError>>,
) -> CognitoAccessTokenValidator<SequenceJwksSource> {
    CognitoAccessTokenValidator::new(
        policy(Duration::from_secs(60)),
        SequenceJwksSource::new(responses),
    )
}

fn token_with_header(encoded_header: &str) -> String {
    let mut parts = VALID_TOKEN.split('.');
    let _original_header = parts.next().unwrap();
    let claims = parts.next().unwrap();
    let signature = parts.next().unwrap();
    format!("{encoded_header}.{claims}.{signature}")
}

fn secret(value: &[u8]) -> SecretBytes {
    SecretBytes::new(value.to_vec())
}

#[test]
fn token_policy_requires_bounded_explicit_values() {
    assert_eq!(
        CognitoTokenPolicy::new("", CLIENT_ID, ["cipher:read"], Duration::from_secs(60))
            .unwrap_err(),
        CognitoTokenPolicyError::InvalidIssuer
    );
    assert_eq!(
        CognitoTokenPolicy::new(
            format!("{ISSUER}\u{00a0}"),
            CLIENT_ID,
            ["cipher:read"],
            Duration::from_secs(60)
        )
        .unwrap_err(),
        CognitoTokenPolicyError::InvalidIssuer
    );
    assert_eq!(
        CognitoTokenPolicy::new(
            ISSUER,
            "client id",
            ["cipher:read"],
            Duration::from_secs(60)
        )
        .unwrap_err(),
        CognitoTokenPolicyError::InvalidClientId
    );
    assert_eq!(
        CognitoTokenPolicy::new(
            ISSUER,
            CLIENT_ID,
            std::iter::empty::<&str>(),
            Duration::from_secs(60)
        )
        .unwrap_err(),
        CognitoTokenPolicyError::MissingRequiredScope
    );
    assert_eq!(
        CognitoTokenPolicy::new(ISSUER, CLIENT_ID, ["cipher read"], Duration::from_secs(60))
            .unwrap_err(),
        CognitoTokenPolicyError::InvalidRequiredScope
    );
    assert_eq!(
        CognitoTokenPolicy::new(
            ISSUER,
            CLIENT_ID,
            (0..=MAX_REQUIRED_SCOPES).map(|index| format!("cipher:scope:{index}")),
            Duration::from_secs(60)
        )
        .unwrap_err(),
        CognitoTokenPolicyError::TooManyRequiredScopes
    );

    for lifetime in [Duration::ZERO, Duration::from_millis(999)] {
        assert_eq!(
            CognitoTokenPolicy::new(ISSUER, CLIENT_ID, ["cipher:read"], lifetime).unwrap_err(),
            CognitoTokenPolicyError::InvalidJwksMaxAge
        );
    }
    assert_eq!(
        CognitoTokenPolicy::new(
            ISSUER,
            CLIENT_ID,
            ["cipher:read"],
            Duration::from_secs(24 * 60 * 60 + 1)
        )
        .unwrap_err(),
        CognitoTokenPolicyError::InvalidJwksMaxAge
    );
}

#[test]
fn public_error_categories_have_fixed_nonempty_messages() {
    for error in [
        CognitoTokenPolicyError::InvalidIssuer,
        CognitoTokenPolicyError::InvalidClientId,
        CognitoTokenPolicyError::InvalidRequiredScope,
        CognitoTokenPolicyError::MissingRequiredScope,
        CognitoTokenPolicyError::TooManyRequiredScopes,
        CognitoTokenPolicyError::InvalidJwksMaxAge,
    ] {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        JwksSourceError::Unavailable,
        JwksSourceError::InvalidResponse,
    ] {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        CognitoTokenValidationError::InvalidClock,
        CognitoTokenValidationError::MalformedToken,
        CognitoTokenValidationError::UnsupportedAlgorithm,
        CognitoTokenValidationError::MissingKeyId,
        CognitoTokenValidationError::KeyUnavailable,
        CognitoTokenValidationError::UnknownKeyId,
        CognitoTokenValidationError::InvalidSignature,
        CognitoTokenValidationError::Expired,
        CognitoTokenValidationError::InvalidIssuer,
        CognitoTokenValidationError::InvalidClientId,
        CognitoTokenValidationError::InvalidTokenUse,
        CognitoTokenValidationError::MissingRequiredScope,
        CognitoTokenValidationError::InvalidSubject,
    ] {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        CognitoChallengeError::InvalidLifetime,
        CognitoChallengeError::InvalidClock,
        CognitoChallengeError::StateIdentifierExhausted,
        CognitoChallengeError::TicketIdentifierExhausted,
        CognitoChallengeError::InvalidContinuation,
        CognitoChallengeError::ChallengePending,
        CognitoChallengeError::UnknownTicket,
        CognitoChallengeError::WrongFlow,
        CognitoChallengeError::Expired,
        CognitoChallengeError::MalformedResponse,
        CognitoChallengeError::Replay,
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn valid_access_token_yields_only_native_redacted_values() {
    let mut validator = validator_with([Ok(alpha_jwks())]);

    let validated = validator.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();

    assert_eq!(validated.subject().as_str(), SUBJECT);
    assert_eq!(validated.expires_at(), TOKEN_EXPIRATION);
    let validated_debug = format!("{validated:?}");
    assert!(!validated_debug.contains(SUBJECT));
    assert!(!validated_debug.contains(VALID_TOKEN));

    let (subject, access_token) = validated.into_parts();
    assert_eq!(subject.as_str(), SUBJECT);
    assert_eq!(format!("{subject:?}"), "CognitoSubject([redacted])");
    assert_eq!(format!("{access_token:?}"), "AccessToken([redacted])");
    assert_eq!(validator.source.calls.get(), 1);

    validator
        .validate_at(VALID_TOKEN.as_bytes(), NOW + 1)
        .unwrap();
    assert_eq!(validator.source.calls.get(), 1);

    let policy_debug = format!("{:?}", validator.policy);
    assert!(!policy_debug.contains(ISSUER));
    assert!(!policy_debug.contains(CLIENT_ID));
    assert!(!format!("{validator:?}").contains(ALPHA_MODULUS));
}

#[test]
fn exact_cognito_claims_are_required_after_signature_validation() {
    for (token, expected) in [
        (
            WRONG_ISSUER_TOKEN,
            CognitoTokenValidationError::InvalidIssuer,
        ),
        (
            WRONG_CLIENT_TOKEN,
            CognitoTokenValidationError::InvalidClientId,
        ),
        (
            WRONG_TOKEN_USE_TOKEN,
            CognitoTokenValidationError::InvalidTokenUse,
        ),
        (EXPIRED_TOKEN, CognitoTokenValidationError::Expired),
        (
            MISSING_SCOPE_TOKEN,
            CognitoTokenValidationError::MissingRequiredScope,
        ),
        (
            DUPLICATE_SCOPE_TOKEN,
            CognitoTokenValidationError::MissingRequiredScope,
        ),
        (
            INVALID_SUBJECT_TOKEN,
            CognitoTokenValidationError::InvalidSubject,
        ),
    ] {
        let mut validator = validator_with([Ok(alpha_jwks())]);
        assert_eq!(
            validator.validate_at(token.as_bytes(), NOW).unwrap_err(),
            expected
        );
    }

    let mut expiration_validator = validator_with([Ok(alpha_jwks())]);
    assert_eq!(
        expiration_validator
            .validate_at(VALID_TOKEN.as_bytes(), TOKEN_EXPIRATION)
            .unwrap_err(),
        CognitoTokenValidationError::Expired
    );
}

#[test]
fn malformed_tokens_algorithms_and_missing_key_ids_fail_before_key_loading() {
    let mut validator = validator_with([]);
    for token in [
        b"".as_slice(),
        b"one.two",
        b"one.two.three.four",
        b"one two.three.four",
    ] {
        assert_eq!(
            validator.validate_at(token, NOW).unwrap_err(),
            CognitoTokenValidationError::MalformedToken
        );
    }
    assert_eq!(
        validator
            .validate_at(VALID_TOKEN.as_bytes(), -1)
            .unwrap_err(),
        CognitoTokenValidationError::InvalidClock
    );
    assert_eq!(validator.source.calls.get(), 0);

    let unsupported = token_with_header("eyJhbGciOiJIUzI1NiIsImtpZCI6ImFscGhhIiwidHlwIjoiSldUIn0");
    assert_eq!(
        validator
            .validate_at(unsupported.as_bytes(), NOW)
            .unwrap_err(),
        CognitoTokenValidationError::UnsupportedAlgorithm
    );
    let missing_key_id = token_with_header("eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9");
    assert_eq!(
        validator
            .validate_at(missing_key_id.as_bytes(), NOW)
            .unwrap_err(),
        CognitoTokenValidationError::MissingKeyId
    );
    assert_eq!(validator.source.calls.get(), 0);

    let oversized = vec![b'a'; MAX_ACCESS_TOKEN_BYTES + 1];
    assert_eq!(
        validator.validate_at(&oversized, NOW).unwrap_err(),
        CognitoTokenValidationError::MalformedToken
    );

    assert_eq!(
        map_jwt_error(jsonwebtoken::errors::Error::from(
            ErrorKind::ExpiredSignature
        )),
        CognitoTokenValidationError::Expired
    );
    assert_eq!(
        map_jwt_error(jsonwebtoken::errors::Error::from(
            ErrorKind::InvalidAlgorithm
        )),
        CognitoTokenValidationError::UnsupportedAlgorithm
    );
}

#[test]
fn access_token_scope_parsing_is_bounded_and_unambiguous() {
    assert!(parse_scopes("").is_none());
    assert!(parse_scopes("cipher:read cipher:read").is_none());
    assert!(parse_scopes("cipher:read\u{00a0}cipher:write").is_none());
    let too_many = (0..=MAX_TOKEN_SCOPES)
        .map(|index| format!("cipher:{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(parse_scopes(&too_many).is_none());
}

#[test]
fn bad_signature_never_triggers_a_refresh_for_a_known_fresh_key() {
    let mut validator = validator_with([Ok(alpha_jwks()), Ok(jwks(&[("alpha", BETA_MODULUS)]))]);
    validator.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();

    assert_eq!(
        validator
            .validate_at(ROTATED_ALPHA_TOKEN.as_bytes(), NOW + 1)
            .unwrap_err(),
        CognitoTokenValidationError::InvalidSignature
    );
    assert_eq!(validator.source.calls.get(), 1);

    let mut changed_signature = VALID_TOKEN.to_owned();
    let final_byte = changed_signature.pop().unwrap();
    changed_signature.push(if final_byte == 'A' { 'B' } else { 'A' });
    assert_eq!(
        validator
            .validate_at(changed_signature.as_bytes(), NOW + 2)
            .unwrap_err(),
        CognitoTokenValidationError::InvalidSignature
    );
    assert_eq!(validator.source.calls.get(), 1);
}

#[test]
fn an_unknown_key_id_refreshes_once_and_accepts_a_rotated_key() {
    let mut validator = validator_with([
        Ok(alpha_jwks()),
        Ok(jwks(&[("alpha", ALPHA_MODULUS), ("beta", BETA_MODULUS)])),
    ]);
    validator.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();

    let rotated = validator
        .validate_at(BETA_TOKEN.as_bytes(), NOW + 1)
        .unwrap();

    assert_eq!(rotated.subject().as_str(), SUBJECT);
    assert_eq!(validator.source.calls.get(), 2);
}

#[test]
fn an_unknown_key_id_fails_closed_after_one_current_reload() {
    let mut validator = validator_with([Ok(alpha_jwks()), Ok(alpha_jwks())]);
    validator.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();

    assert_eq!(
        validator
            .validate_at(BETA_TOKEN.as_bytes(), NOW + 1)
            .unwrap_err(),
        CognitoTokenValidationError::UnknownKeyId
    );
    assert_eq!(validator.source.calls.get(), 2);
}

#[test]
fn a_stale_matching_key_must_reload_and_never_falls_back() {
    let mut unavailable = validator_with([Ok(alpha_jwks()), Err(JwksSourceError::Unavailable)]);
    unavailable
        .validate_at(VALID_TOKEN.as_bytes(), NOW)
        .unwrap();
    assert_eq!(
        unavailable
            .validate_at(VALID_TOKEN.as_bytes(), NOW + 60)
            .unwrap_err(),
        CognitoTokenValidationError::KeyUnavailable
    );
    assert_eq!(unavailable.source.calls.get(), 2);

    let mut rotated = validator_with([Ok(alpha_jwks()), Ok(jwks(&[("alpha", BETA_MODULUS)]))]);
    rotated.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();
    rotated
        .validate_at(ROTATED_ALPHA_TOKEN.as_bytes(), NOW + 60)
        .unwrap();
    assert_eq!(rotated.source.calls.get(), 2);
}

#[test]
fn clock_rollback_forces_a_current_key_reload() {
    let mut validator = validator_with([Ok(alpha_jwks()), Ok(alpha_jwks())]);
    validator.validate_at(VALID_TOKEN.as_bytes(), NOW).unwrap();
    validator
        .validate_at(VALID_TOKEN.as_bytes(), NOW - 1)
        .unwrap();
    assert_eq!(validator.source.calls.get(), 2);
}

#[test]
fn malformed_or_unavailable_jwks_never_populates_the_cache() {
    let too_many_keys = jwks(
        &(0..=MAX_JWKS_KEYS)
            .map(|index| (format!("key-{index}"), ALPHA_MODULUS))
            .collect::<Vec<_>>()
            .iter()
            .map(|(key, modulus)| (key.as_str(), *modulus))
            .collect::<Vec<_>>(),
    );
    let invalid_documents = [
        String::new(),
        "{}".to_owned(),
        json!({"keys": []}).to_string(),
        json!({"keys": [{
            "kid": "alpha", "kty": "EC", "alg": "RS256", "use": "sig",
            "n": ALPHA_MODULUS, "e": RSA_EXPONENT
        }]})
        .to_string(),
        json!({"keys": [{
            "kid": "alpha", "kty": "RSA", "alg": "RS256", "use": "sig",
            "n": "not+base64url", "e": RSA_EXPONENT
        }]})
        .to_string(),
        jwks(&[("alpha", ALPHA_MODULUS), ("alpha", BETA_MODULUS)]),
        too_many_keys,
        "x".repeat(MAX_JWKS_BYTES + 1),
    ];

    for document in invalid_documents {
        let mut validator = validator_with([Ok(document)]);
        assert_eq!(
            validator
                .validate_at(VALID_TOKEN.as_bytes(), NOW)
                .unwrap_err(),
            CognitoTokenValidationError::KeyUnavailable
        );
        assert!(validator.cache.is_none());
    }

    let mut unavailable = validator_with([Err(JwksSourceError::InvalidResponse)]);
    assert_eq!(
        unavailable
            .validate_at(VALID_TOKEN.as_bytes(), NOW)
            .unwrap_err(),
        CognitoTokenValidationError::KeyUnavailable
    );
    assert!(unavailable.cache.is_none());
}

#[test]
fn challenge_state_requires_a_bounded_whole_second_lifetime() {
    for lifetime in [Duration::ZERO, Duration::from_millis(999)] {
        assert_eq!(
            CognitoChallengeState::new(lifetime).unwrap_err(),
            CognitoChallengeError::InvalidLifetime
        );
    }
    assert_eq!(
        CognitoChallengeState::new(MAX_CHALLENGE_LIFETIME + Duration::from_secs(1)).unwrap_err(),
        CognitoChallengeError::InvalidLifetime
    );
}

#[test]
fn every_supported_challenge_flow_consumes_its_continuation_once() {
    for flow in [
        CognitoChallengeFlow::EmailVerification,
        CognitoChallengeFlow::PasswordReset,
        CognitoChallengeFlow::SoftwareTokenMfa,
    ] {
        let mut state = CognitoChallengeState::new(Duration::from_secs(120)).unwrap();
        let ticket = state
            .begin(flow, secret(b"opaque-cognito-session"), NOW)
            .unwrap();
        assert_eq!(ticket.flow(), flow);
        assert_eq!(ticket.expires_at(), NOW + 120);

        let response = match flow {
            CognitoChallengeFlow::EmailVerification => {
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456"),
                }
            }
            CognitoChallengeFlow::PasswordReset => CognitoChallengeResponse::PasswordReset {
                code: secret(b"234567"),
                new_password: secret(b"new-password-value"),
            },
            CognitoChallengeFlow::SoftwareTokenMfa => CognitoChallengeResponse::SoftwareTokenMfa {
                code: secret(b"345678"),
            },
        };
        let resolution = state.complete(ticket, response, NOW + 1).unwrap();
        assert_eq!(resolution.flow(), flow);
        let (resolved_flow, continuation, resolved_response) = resolution.into_parts();
        assert_eq!(resolved_flow, flow);
        assert_eq!(continuation.as_bytes(), b"opaque-cognito-session");
        match resolved_response {
            CognitoChallengeResponse::EmailVerification { code }
            | CognitoChallengeResponse::SoftwareTokenMfa { code } => {
                assert_eq!(code.as_bytes().len(), 6);
            }
            CognitoChallengeResponse::PasswordReset { code, new_password } => {
                assert_eq!(code.as_bytes(), b"234567");
                assert_eq!(new_password.as_bytes(), b"new-password-value");
            }
        }

        assert_eq!(
            state
                .complete(
                    ticket,
                    CognitoChallengeResponse::EmailVerification {
                        code: secret(b"123456")
                    },
                    NOW + 2
                )
                .unwrap_err(),
            CognitoChallengeError::Replay
        );
    }
}

#[test]
fn wrong_flow_and_malformed_responses_leave_a_live_ticket_retryable() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let ticket = state
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"verification-session"),
            NOW,
        )
        .unwrap();

    assert_eq!(
        state
            .complete(
                ticket,
                CognitoChallengeResponse::SoftwareTokenMfa {
                    code: secret(b"123456")
                },
                NOW + 1
            )
            .unwrap_err(),
        CognitoChallengeError::WrongFlow
    );
    assert_eq!(
        state
            .complete(
                ticket,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"12a456")
                },
                NOW + 2
            )
            .unwrap_err(),
        CognitoChallengeError::MalformedResponse
    );
    state
        .complete(
            ticket,
            CognitoChallengeResponse::EmailVerification {
                code: secret(b"123456"),
            },
            NOW + 3,
        )
        .unwrap();
}

#[test]
fn password_reset_rejects_unbounded_or_control_character_passwords() {
    for new_password in [
        Vec::new(),
        vec![b'p'; MAX_NEW_PASSWORD_BYTES + 1],
        b"line\nbreak".to_vec(),
        "unicode\u{0085}control".as_bytes().to_vec(),
        vec![0xff],
    ] {
        let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
        let ticket = state
            .begin(
                CognitoChallengeFlow::PasswordReset,
                secret(b"reset-session"),
                NOW,
            )
            .unwrap();
        assert_eq!(
            state
                .complete(
                    ticket,
                    CognitoChallengeResponse::PasswordReset {
                        code: secret(b"123456"),
                        new_password: SecretBytes::new(new_password),
                    },
                    NOW + 1,
                )
                .unwrap_err(),
            CognitoChallengeError::MalformedResponse
        );
    }
}

#[test]
fn expiry_discards_continuations_before_flow_or_shape_checks() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let ticket = state
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"verification-session"),
            NOW,
        )
        .unwrap();

    assert_eq!(
        state
            .complete(
                ticket,
                CognitoChallengeResponse::SoftwareTokenMfa {
                    code: secret(b"invalid")
                },
                ticket.expires_at()
            )
            .unwrap_err(),
        CognitoChallengeError::Expired
    );
    assert!(state.pending.is_none());
    assert_eq!(
        state
            .complete(
                ticket,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                ticket.expires_at() + 1
            )
            .unwrap_err(),
        CognitoChallengeError::Replay
    );
}

#[test]
fn beginning_a_new_flow_retires_an_expired_pending_ticket() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(30)).unwrap();
    let expired = state
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"expired-session"),
            NOW,
        )
        .unwrap();
    assert_eq!(
        state
            .begin(
                CognitoChallengeFlow::SoftwareTokenMfa,
                secret(b""),
                NOW + 30
            )
            .unwrap_err(),
        CognitoChallengeError::InvalidContinuation
    );
    assert_eq!(
        state
            .complete(
                expired,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                NOW + 30
            )
            .unwrap_err(),
        CognitoChallengeError::Replay
    );
    let current = state
        .begin(
            CognitoChallengeFlow::SoftwareTokenMfa,
            secret(b"current-session"),
            NOW + 30,
        )
        .unwrap();

    state
        .complete(
            current,
            CognitoChallengeResponse::SoftwareTokenMfa {
                code: secret(b"654321"),
            },
            NOW + 31,
        )
        .unwrap();
}

#[test]
fn active_challenges_reject_replacement_and_cross_state_tickets() {
    let mut first = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let mut second = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let first_ticket = first
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"first-session"),
            NOW,
        )
        .unwrap();
    assert_eq!(
        first
            .begin(
                CognitoChallengeFlow::PasswordReset,
                secret(b"replacement-session"),
                NOW + 1
            )
            .unwrap_err(),
        CognitoChallengeError::ChallengePending
    );
    let second_ticket = second
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"second-session"),
            NOW,
        )
        .unwrap();

    let mut unknown_ticket = first_ticket;
    unknown_ticket.sequence += 1;
    assert_eq!(
        first
            .complete(
                unknown_ticket,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                NOW + 1
            )
            .unwrap_err(),
        CognitoChallengeError::UnknownTicket
    );
    assert_eq!(
        first
            .complete(
                first_ticket,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                -1
            )
            .unwrap_err(),
        CognitoChallengeError::InvalidClock
    );

    assert_eq!(
        first
            .complete(
                second_ticket,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                NOW + 1
            )
            .unwrap_err(),
        CognitoChallengeError::UnknownTicket
    );
    first
        .complete(
            first_ticket,
            CognitoChallengeResponse::EmailVerification {
                code: secret(b"123456"),
            },
            NOW + 2,
        )
        .unwrap();
}

#[test]
fn retired_ticket_history_stays_replay_safe_without_unbounded_storage() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let first = state
        .begin(
            CognitoChallengeFlow::EmailVerification,
            secret(b"first-session"),
            NOW,
        )
        .unwrap();
    state
        .complete(
            first,
            CognitoChallengeResponse::EmailVerification {
                code: secret(b"123456"),
            },
            NOW + 1,
        )
        .unwrap();
    let second = state
        .begin(
            CognitoChallengeFlow::SoftwareTokenMfa,
            secret(b"second-session"),
            NOW + 2,
        )
        .unwrap();
    state
        .complete(
            second,
            CognitoChallengeResponse::SoftwareTokenMfa {
                code: secret(b"654321"),
            },
            NOW + 3,
        )
        .unwrap();

    assert_eq!(
        state
            .complete(
                first,
                CognitoChallengeResponse::EmailVerification {
                    code: secret(b"123456")
                },
                NOW + 4
            )
            .unwrap_err(),
        CognitoChallengeError::Replay
    );
    assert_eq!(state.retired_through, second.sequence);
}

#[test]
fn challenge_inputs_and_timestamps_are_bounded_and_fail_closed() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    assert_eq!(
        state
            .begin(CognitoChallengeFlow::EmailVerification, secret(b""), NOW)
            .unwrap_err(),
        CognitoChallengeError::InvalidContinuation
    );
    assert_eq!(
        state
            .begin(
                CognitoChallengeFlow::EmailVerification,
                SecretBytes::new(vec![b'x'; MAX_CHALLENGE_CONTINUATION_BYTES + 1]),
                NOW
            )
            .unwrap_err(),
        CognitoChallengeError::InvalidContinuation
    );
    assert_eq!(
        state
            .begin(
                CognitoChallengeFlow::EmailVerification,
                secret(b"session"),
                -1
            )
            .unwrap_err(),
        CognitoChallengeError::InvalidClock
    );
    assert_eq!(
        state
            .begin(
                CognitoChallengeFlow::EmailVerification,
                secret(b"session"),
                i64::MAX
            )
            .unwrap_err(),
        CognitoChallengeError::InvalidClock
    );
    assert_eq!(state.next_ticket_sequence, 1);

    state.next_ticket_sequence = u64::MAX;
    assert_eq!(
        state
            .begin(
                CognitoChallengeFlow::EmailVerification,
                secret(b"session"),
                NOW
            )
            .unwrap_err(),
        CognitoChallengeError::TicketIdentifierExhausted
    );
}

#[test]
fn challenge_debug_output_never_contains_tickets_or_secret_values() {
    let mut state = CognitoChallengeState::new(Duration::from_secs(60)).unwrap();
    let ticket = state
        .begin(
            CognitoChallengeFlow::PasswordReset,
            secret(b"private-continuation"),
            NOW,
        )
        .unwrap();
    let response = CognitoChallengeResponse::PasswordReset {
        code: secret(b"123456"),
        new_password: secret(b"private-password"),
    };

    assert_eq!(format!("{ticket:?}"), "CognitoChallengeTicket([redacted])");
    assert_eq!(
        format!("{response:?}"),
        "CognitoChallengeResponse([redacted])"
    );
    let state_debug = format!("{state:?}");
    assert!(!state_debug.contains("private-continuation"));
    assert!(!state_debug.contains(&ticket.state_id.to_string()));

    let resolution = state.complete(ticket, response, NOW + 1).unwrap();
    assert_eq!(
        format!("{resolution:?}"),
        "CognitoChallengeResolution([redacted])"
    );
}
