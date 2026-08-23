//! Cognito-compatible SRP-6a proof construction.

use std::{collections::HashMap, fmt};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, KeyInit, Mac};
use sha2_next::{Digest, Sha256};
use srp::{
    Client, EphemeralSecret, Generate, Group,
    bigint::{
        BoxedUint, U3072,
        modular::{ConstMontyForm, ConstMontyParams, FixedMontyParams},
    },
};
use zeroize::Zeroizing;

const MAX_CHALLENGE_VALUE_BYTES: usize = 16 * 1024;
const COGNITO_MODULUS_BITS: u32 = 3_072;
const COGNITO_MODULUS_BYTES: usize = 384;
const KEY_DERIVATION_INFO: &[u8] = b"Caldera Derived Key\x01";
const COGNITO_MODULUS_HEX: &str = "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca18217c32905e462e36ce3be39e772c180e86039b2783a2ec07a28fb5c55df06f4c52c9de2bcbf6955817183995497cea956ae515d2261898fa051015728e5a8aaac42dad33170d04507a33a85521abdf1cba64ecfb850458dbef0a8aea71575d060c7db3970f85a6e1e4c7abf5ae8cdb0933d71e8c94e04a25619dcee3d2261ad2ee6bf12ffa06d98a0864d87602733ec86a64521f2b18177b200cbbe117577a615d6c770988c0bad946e208e24fa074e5ab3143db5bfce0fd108e4b82d120a93ad2caffffffffffffffff";

type CognitoSrpClient = Client<CognitoGroup, Sha256>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct CognitoGroup;

impl ConstMontyParams<{ U3072::LIMBS }> for CognitoGroup {
    const LIMBS: usize = U3072::LIMBS;
    const PARAMS: FixedMontyParams<{ U3072::LIMBS }> =
        FixedMontyParams::new_vartime(srp::bigint::Odd::<U3072>::from_be_hex(COGNITO_MODULUS_HEX));
}

impl Group for CognitoGroup {
    type Element = ConstMontyForm<Self, { U3072::LIMBS }>;
    const G: Self::Element = ConstMontyForm::new(&U3072::from_u128(2));
}

/// A short-lived SRP exchange that borrows credentials from a zeroizing request.
pub(super) struct CognitoSrpExchange<'credential> {
    client: CognitoSrpClient,
    ephemeral: Zeroizing<Vec<u8>>,
    identifier: &'credential str,
    password: &'credential str,
    pool_name: String,
    public_ephemeral: String,
}

impl<'credential> CognitoSrpExchange<'credential> {
    /// Starts an exchange with a freshly generated private ephemeral value.
    pub(super) fn begin(
        pool_id: &str,
        identifier: &'credential str,
        password: &'credential str,
    ) -> Result<Self, CognitoSrpError> {
        Self::with_ephemeral(
            pool_id,
            identifier,
            password,
            EphemeralSecret::generate().to_vec(),
        )
    }

    fn with_ephemeral(
        pool_id: &str,
        identifier: &'credential str,
        password: &'credential str,
        ephemeral: Vec<u8>,
    ) -> Result<Self, CognitoSrpError> {
        let ephemeral = Zeroizing::new(ephemeral);
        let pool_name = pool_id
            .split_once('_')
            .map(|(_, pool_name)| pool_name)
            .filter(|pool_name| !pool_name.is_empty())
            .ok_or(CognitoSrpError::InvalidConfiguration)?
            .to_owned();
        let client = CognitoSrpClient::new();
        if ephemeral.len() < 32 || ephemeral.len() > COGNITO_MODULUS_BYTES {
            return Err(CognitoSrpError::ProofFailure);
        }
        let private_ephemeral = Zeroizing::new(
            BoxedUint::from_be_slice(ephemeral.as_ref(), COGNITO_MODULUS_BITS)
                .map_err(|_| CognitoSrpError::ProofFailure)?,
        );
        // The SRP crate owns unavoidable arithmetic temporaries inside this call;
        // the private input and every secret result retained here are zeroized.
        let public_ephemeral = hex::encode(
            client
                .compute_g_x(&private_ephemeral)
                .to_be_bytes_trimmed_vartime(),
        );
        Ok(Self {
            client,
            ephemeral,
            identifier,
            password,
            pool_name,
            public_ephemeral,
        })
    }

    /// Returns the public parameters for `USER_SRP_AUTH`.
    pub(super) fn initial_parameters(&self) -> HashMap<String, String> {
        // Cognito's SDK requires owned request strings. The provider moves these
        // buffers directly into its one-attempt request and does not retain them.
        HashMap::from([
            ("USERNAME".to_owned(), self.identifier.to_owned()),
            ("SRP_A".to_owned(), self.public_ephemeral.clone()),
        ])
    }

    /// Produces the Cognito `PASSWORD_VERIFIER` response without retaining proof material.
    pub(super) fn password_verifier(
        &self,
        parameters: &HashMap<String, String>,
        timestamp: &str,
    ) -> Result<HashMap<String, String>, CognitoSrpError> {
        let secret_block = required_parameter(parameters, "SECRET_BLOCK")?;
        let user_id = required_parameter(parameters, "USER_ID_FOR_SRP")?;
        let salt = decode_hex(required_parameter(parameters, "SALT")?)?;
        let public_b_int = parse_public_b(required_parameter(parameters, "SRP_B")?)?;
        let secret_block_bytes = Zeroizing::new(
            BASE64
                .decode(secret_block)
                .map_err(|_| CognitoSrpError::MalformedChallenge)?,
        );
        if secret_block_bytes.is_empty() || secret_block_bytes.len() > MAX_CHALLENGE_VALUE_BYTES {
            return Err(CognitoSrpError::MalformedChallenge);
        }

        let public_a = decode_hex(&self.public_ephemeral)?;
        let public_b = public_b_int.to_be_bytes_trimmed_vartime();
        let scrambling = hash_integer(&[
            signed_integer_bytes(&public_a).as_slice(),
            signed_integer_bytes(&public_b).as_slice(),
        ]);
        if bool::from(scrambling.is_zero()) {
            return Err(CognitoSrpError::MalformedChallenge);
        }

        let mut identity = Sha256::new();
        identity.update(self.pool_name.as_bytes());
        identity.update(user_id.as_bytes());
        identity.update(b":");
        identity.update(self.password.as_bytes());
        let identity = Zeroizing::new(identity.finalize().to_vec());
        let private_key = Zeroizing::new(hash_integer(&[
            signed_integer_bytes(&salt).as_slice(),
            &identity,
        ]));
        let multiplier = cognito_multiplier();
        let ephemeral = Zeroizing::new(
            BoxedUint::from_be_slice(self.ephemeral.as_ref(), COGNITO_MODULUS_BITS)
                .map_err(|_| CognitoSrpError::ProofFailure)?,
        );
        let premaster = Zeroizing::new(self.client.compute_premaster_secret(
            &public_b_int,
            &multiplier,
            &private_key,
            &ephemeral,
            &scrambling,
        ));
        let premaster_bytes = Zeroizing::new(premaster.to_be_bytes_trimmed_vartime().to_vec());

        let scrambling_bytes = scrambling.to_be_bytes_trimmed_vartime();
        let mut extract = HmacSha256::new_from_slice(&signed_integer_bytes(&scrambling_bytes))
            .map_err(|_| CognitoSrpError::ProofFailure)?;
        let signed_premaster = Zeroizing::new(signed_integer_bytes(&premaster_bytes));
        extract.update(&signed_premaster);
        let extracted = Zeroizing::new(extract.finalize().into_bytes().to_vec());

        let mut expand =
            HmacSha256::new_from_slice(&extracted).map_err(|_| CognitoSrpError::ProofFailure)?;
        expand.update(KEY_DERIVATION_INFO);
        let expanded = Zeroizing::new(expand.finalize().into_bytes().to_vec());
        let key = expanded.get(..16).ok_or(CognitoSrpError::ProofFailure)?;

        let mut claim =
            HmacSha256::new_from_slice(key).map_err(|_| CognitoSrpError::ProofFailure)?;
        claim.update(self.pool_name.as_bytes());
        claim.update(user_id.as_bytes());
        claim.update(&secret_block_bytes);
        claim.update(timestamp.as_bytes());
        let signature = BASE64.encode(claim.finalize().into_bytes());

        // The proof and returned secret block must become SDK-owned request
        // strings. The provider moves this map directly into the request.
        Ok(HashMap::from([
            ("USERNAME".to_owned(), user_id.to_owned()),
            (
                "PASSWORD_CLAIM_SECRET_BLOCK".to_owned(),
                secret_block.to_owned(),
            ),
            ("PASSWORD_CLAIM_SIGNATURE".to_owned(), signature),
            ("TIMESTAMP".to_owned(), timestamp.to_owned()),
        ]))
    }
}

impl fmt::Debug for CognitoSrpExchange<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CognitoSrpExchange")
            .field("secret", &"[redacted]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CognitoSrpError {
    InvalidConfiguration,
    MalformedChallenge,
    ProofFailure,
}

fn required_parameter<'a>(
    parameters: &'a HashMap<String, String>,
    name: &str,
) -> Result<&'a str, CognitoSrpError> {
    parameters
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_CHALLENGE_VALUE_BYTES)
        .ok_or(CognitoSrpError::MalformedChallenge)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, CognitoSrpError> {
    if value.is_empty() || value.len() > MAX_CHALLENGE_VALUE_BYTES * 2 {
        return Err(CognitoSrpError::MalformedChallenge);
    }
    let padded;
    let value = if value.len().is_multiple_of(2) {
        value
    } else {
        padded = format!("0{value}");
        &padded
    };
    hex::decode(value).map_err(|_| CognitoSrpError::MalformedChallenge)
}

fn parse_public_b(value: &str) -> Result<BoxedUint, CognitoSrpError> {
    // Cognito encodes positive big integers as signed values. A full-width
    // 3072-bit public value whose first bit is set therefore carries one
    // leading `00` sign byte (770 hexadecimal characters), which must be
    // accepted without allowing a wider modulus value.
    if value.is_empty() || value.len() > (COGNITO_MODULUS_BYTES + 1) * 2 {
        return Err(CognitoSrpError::MalformedChallenge);
    }
    let bytes = decode_hex(value)?;
    let magnitude = match bytes.as_slice() {
        [0, magnitude @ ..] if magnitude.len() <= COGNITO_MODULUS_BYTES => magnitude,
        magnitude if magnitude.len() <= COGNITO_MODULUS_BYTES => magnitude,
        _ => return Err(CognitoSrpError::MalformedChallenge),
    };
    let public_b = BoxedUint::from_be_slice(magnitude, COGNITO_MODULUS_BITS)
        .map_err(|_| CognitoSrpError::MalformedChallenge)?;
    validate_public_b(&public_b)?;
    Ok(public_b)
}

fn signed_integer_bytes(value: &[u8]) -> Vec<u8> {
    if value.first().is_some_and(|first| first & 0x80 != 0) {
        let mut padded = Vec::with_capacity(value.len() + 1);
        padded.push(0);
        padded.extend_from_slice(value);
        padded
    } else {
        value.to_vec()
    }
}

fn hash_integer(parts: &[&[u8]]) -> BoxedUint {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    BoxedUint::from_be_slice_vartime(&digest.finalize())
}

fn cognito_multiplier() -> BoxedUint {
    let modulus = CognitoGroup::generator().params().modulus().to_be_bytes();
    hash_integer(&[&[0], &modulus, &[2]])
}

fn validate_public_b(public_b: &BoxedUint) -> Result<(), CognitoSrpError> {
    let generator = CognitoGroup::generator();
    let modulus = generator.params().modulus().as_nz_ref();
    if bool::from(public_b.is_zero()) || !public_b.cmp_vartime(&**modulus).is_lt() {
        Err(CognitoSrpError::MalformedChallenge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::Deserialize;

    use super::{
        COGNITO_MODULUS_BYTES, COGNITO_MODULUS_HEX, CognitoSrpError, CognitoSrpExchange,
        parse_public_b, signed_integer_bytes,
    };

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReferenceVector {
        account_provisioning: String,
        pool_id: String,
        sign_in_alias: String,
        temporary_password: String,
        ephemeral_byte: u8,
        ephemeral_length: usize,
        public_a: String,
        secret_block: String,
        canonical_username: String,
        salt: String,
        public_b: String,
        timestamp: String,
        signature: String,
    }

    fn reference_vector() -> ReferenceVector {
        serde_json::from_str(include_str!(
            "../../../contracts/auth/v1/cognito-srp-reference.json"
        ))
        .unwrap()
    }

    fn challenge(vector: &ReferenceVector) -> HashMap<String, String> {
        HashMap::from([
            ("SECRET_BLOCK".into(), vector.secret_block.clone()),
            ("USER_ID_FOR_SRP".into(), vector.canonical_username.clone()),
            ("SALT".into(), vector.salt.clone()),
            ("SRP_B".into(), vector.public_b.clone()),
        ])
    }

    #[test]
    fn matches_the_cognito_srp_reference_vector() {
        let vector = reference_vector();
        let exchange = CognitoSrpExchange::with_ephemeral(
            &vector.pool_id,
            &vector.sign_in_alias,
            &vector.temporary_password,
            vec![vector.ephemeral_byte; vector.ephemeral_length],
        )
        .unwrap();
        assert_eq!(vector.account_provisioning, "AdminCreateUser");
        assert_eq!(
            exchange.initial_parameters(),
            HashMap::from([
                ("USERNAME".into(), vector.sign_in_alias.clone()),
                ("SRP_A".into(), vector.public_a.clone()),
            ])
        );
        let response = exchange
            .password_verifier(&challenge(&vector), &vector.timestamp)
            .unwrap();

        assert_eq!(response.len(), 4);
        assert_eq!(response["USERNAME"], vector.canonical_username);
        assert_eq!(response["PASSWORD_CLAIM_SECRET_BLOCK"], vector.secret_block);
        assert_eq!(response["TIMESTAMP"], vector.timestamp);
        assert_eq!(response["PASSWORD_CLAIM_SIGNATURE"], vector.signature);
    }

    #[test]
    fn rejects_missing_malformed_and_malicious_challenge_values() {
        let vector = reference_vector();
        let exchange = CognitoSrpExchange::with_ephemeral(
            &vector.pool_id,
            &vector.sign_in_alias,
            &vector.temporary_password,
            vec![vector.ephemeral_byte; vector.ephemeral_length],
        )
        .unwrap();

        let mut missing = challenge(&vector);
        missing.remove("SALT");
        assert_eq!(
            exchange.password_verifier(&missing, &vector.timestamp),
            Err(CognitoSrpError::MalformedChallenge)
        );

        let mut malformed = challenge(&vector);
        malformed.insert("SECRET_BLOCK".into(), "not-base64".into());
        assert_eq!(
            exchange.password_verifier(&malformed, &vector.timestamp),
            Err(CognitoSrpError::MalformedChallenge)
        );

        let mut zero_public_b = challenge(&vector);
        zero_public_b.insert("SRP_B".into(), "00".into());
        assert_eq!(
            exchange.password_verifier(&zero_public_b, &vector.timestamp),
            Err(CognitoSrpError::MalformedChallenge)
        );

        let mut modulus_public_b = challenge(&vector);
        modulus_public_b.insert("SRP_B".into(), COGNITO_MODULUS_HEX.into());
        assert_eq!(
            exchange.password_verifier(&modulus_public_b, &vector.timestamp),
            Err(CognitoSrpError::MalformedChallenge)
        );

        let mut oversized_public_b = challenge(&vector);
        oversized_public_b.insert("SRP_B".into(), format!("00{COGNITO_MODULUS_HEX}"));
        assert_eq!(
            exchange.password_verifier(&oversized_public_b, &vector.timestamp),
            Err(CognitoSrpError::MalformedChallenge)
        );
    }

    #[test]
    fn accepts_canonical_leading_zero_high_bit_and_full_width_public_values() {
        let vector = reference_vector();
        let exchange = CognitoSrpExchange::with_ephemeral(
            &vector.pool_id,
            &vector.sign_in_alias,
            &vector.temporary_password,
            vec![vector.ephemeral_byte; vector.ephemeral_length],
        )
        .unwrap();

        let mut leading_zero = challenge(&vector);
        leading_zero.insert("SRP_B".into(), format!("00{}", vector.public_b));
        let response = exchange
            .password_verifier(&leading_zero, &vector.timestamp)
            .unwrap();
        assert_eq!(response["PASSWORD_CLAIM_SIGNATURE"], vector.signature);

        let mut high_bit = challenge(&vector);
        high_bit.insert("SRP_B".into(), "80".into());
        assert!(
            exchange
                .password_verifier(&high_bit, &vector.timestamp)
                .is_ok()
        );

        let full_width = format!("80{}", "00".repeat(COGNITO_MODULUS_BYTES - 1));
        assert_eq!(full_width.len(), COGNITO_MODULUS_BYTES * 2);
        assert!(parse_public_b(&full_width).is_ok());
        let signed_full_width = format!("00{full_width}");
        assert_eq!(signed_full_width.len(), (COGNITO_MODULUS_BYTES + 1) * 2);
        assert!(parse_public_b(&signed_full_width).is_ok());
        let mut full_width_challenge = challenge(&vector);
        full_width_challenge.insert("SRP_B".into(), signed_full_width);
        assert!(
            exchange
                .password_verifier(&full_width_challenge, &vector.timestamp)
                .is_ok()
        );

        assert!(parse_public_b(&"ff".repeat(COGNITO_MODULUS_BYTES)).is_err());
    }

    #[test]
    fn validates_configuration_signed_padding_and_redacted_debug() {
        let vector = reference_vector();
        assert_eq!(signed_integer_bytes(&[0x7f]), vec![0x7f]);
        assert_eq!(signed_integer_bytes(&[0x80]), vec![0, 0x80]);
        assert_eq!(
            CognitoSrpExchange::with_ephemeral(
                "missing-separator",
                &vector.sign_in_alias,
                &vector.temporary_password,
                vec![1; 48],
            )
            .unwrap_err(),
            CognitoSrpError::InvalidConfiguration
        );

        let exchange = CognitoSrpExchange::with_ephemeral(
            "us-east-1_pool",
            &vector.sign_in_alias,
            &vector.temporary_password,
            vec![1; 48],
        )
        .unwrap();
        let debug = format!("{exchange:?}");
        assert!(!debug.contains(&vector.sign_in_alias));
        assert!(!debug.contains(&vector.temporary_password));
    }
}
