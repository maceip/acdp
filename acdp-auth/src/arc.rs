//! Anonymous Rate-Limited Credentials (ARC)
//!
//! Based on IETF draft-yun-cfrg-arc using CMZ14 MACGGM construction.
//! Implementation follows Apple's Swift Crypto for compatibility.

use crate::error::{ACDPError, Result};
use arrayref::array_ref;
use ff::PrimeField;
use p256::{
    elliptic_curve::{
        group::{Group, GroupEncoding},
        hash2curve::{ExpandMsgXmd, GroupDigest},
        Field, FieldBytes,
    },
    AffinePoint, NistP256, ProjectivePoint, Scalar,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::atomic::{AtomicU64, Ordering};

/// ARC Ciphersuite for P-256
///
/// Matches Swift Crypto's ARCV1-P256
pub struct ARCCiphersuite {
    pub suite_id: u16,
    pub domain: &'static str,
    pub scalar_byte_count: usize,
    pub point_byte_count: usize,
}

impl ARCCiphersuite {
    /// P-256 ciphersuite (Suite ID 3)
    pub const P256: Self = Self {
        suite_id: 3,
        domain: "ARCV1-P256",
        scalar_byte_count: 32,
        point_byte_count: 33, // Compressed point
    };
}

/// ARC Generators (G, H)
///
/// G = standard P-256 generator
/// H = HashToGroup(G, "HashToGroup-ARCV1-P256generatorH")
pub struct ARCGenerators {
    pub g: ProjectivePoint,
    pub h: ProjectivePoint,
}

impl ARCGenerators {
    /// Compute generators for P-256 ciphersuite
    pub fn new() -> Self {
        let g = ProjectivePoint::GENERATOR;

        // H = HashToGroup(G.bytes, "HashToGroup-ARCV1-P256generatorH")
        let dst = b"HashToGroup-ARCV1-P256generatorH";
        let g_bytes = g.to_affine().to_bytes();
        let h = NistP256::hash_from_bytes::<ExpandMsgXmd<Sha256>>(&[&g_bytes], &[dst])
            .expect("hash-to-curve should not fail");

        Self { g, h }
    }
}

impl Default for ARCGenerators {
    fn default() -> Self {
        Self::new()
    }
}

/// Server Private Key
///
/// Scalars (x0, x1, x2) used for credential issuance and verification
#[derive(Clone)]
pub struct ServerPrivateKey {
    pub x0: Scalar,
    pub x1: Scalar,
    pub x2: Scalar,
    pub x0_blinding: Scalar, // For blinded issuance
}

impl ServerPrivateKey {
    /// Generate random server private key
    pub fn random() -> Self {
        use rand_core::OsRng;
        Self {
            x0: Scalar::random(&mut OsRng),
            x1: Scalar::random(&mut OsRng),
            x2: Scalar::random(&mut OsRng),
            x0_blinding: Scalar::random(&mut OsRng),
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(&self.x0.to_bytes());
        bytes.extend_from_slice(&self.x1.to_bytes());
        bytes.extend_from_slice(&self.x2.to_bytes());
        bytes.extend_from_slice(&self.x0_blinding.to_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 128 {
            return Err(ACDPError::ARCVerificationFailed(
                "Invalid server private key length".to_string(),
            ));
        }

        let x0_bytes = FieldBytes::<NistP256>::from(*array_ref![bytes, 0, 32]);
        let x0: Option<Scalar> = Scalar::from_repr(x0_bytes).into();
        let x0 =
            x0.ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid x0 scalar".to_string()))?;

        let x1_bytes = FieldBytes::<NistP256>::from(*array_ref![bytes, 32, 32]);
        let x1: Option<Scalar> = Scalar::from_repr(x1_bytes).into();
        let x1 =
            x1.ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid x1 scalar".to_string()))?;

        let x2_bytes = FieldBytes::<NistP256>::from(*array_ref![bytes, 64, 32]);
        let x2: Option<Scalar> = Scalar::from_repr(x2_bytes).into();
        let x2 =
            x2.ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid x2 scalar".to_string()))?;

        let x0_blinding_bytes = FieldBytes::<NistP256>::from(*array_ref![bytes, 96, 32]);
        let x0_blinding: Option<Scalar> = Scalar::from_repr(x0_blinding_bytes).into();
        let x0_blinding = x0_blinding.ok_or_else(|| {
            ACDPError::ARCVerificationFailed("Invalid x0_blinding scalar".to_string())
        })?;

        Ok(Self {
            x0,
            x1,
            x2,
            x0_blinding,
        })
    }
}

/// Server Public Key
///
/// Commitments to server private key: X0, X1, X2
#[derive(Clone, Debug)]
pub struct ServerPublicKey {
    pub x0: ProjectivePoint,
    pub x1: ProjectivePoint,
    pub x2: ProjectivePoint,
}

impl ServerPublicKey {
    /// Derive from private key
    /// Following CMZ14 MACGGM pattern:
    /// X[0] = x0_blinding*G + x0*H (dual generator commitment)
    /// X[i] = x[i]*G for i > 0
    pub fn from_private_key(private_key: &ServerPrivateKey, generators: &ARCGenerators) -> Self {
        Self {
            // X0 uses both generators (CMZ14 pattern)
            x0: generators.g * private_key.x0_blinding + generators.h * private_key.x0,
            // X1, X2 use only G
            x1: generators.g * private_key.x1,
            x2: generators.g * private_key.x2,
        }
    }

    /// Serialize to bytes (compressed points)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(99); // 3 * 33 bytes
        bytes.extend_from_slice(&self.x0.to_affine().to_bytes());
        bytes.extend_from_slice(&self.x1.to_affine().to_bytes());
        bytes.extend_from_slice(&self.x2.to_affine().to_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 99 {
            return Err(ACDPError::ARCVerificationFailed(
                "Invalid server public key length".to_string(),
            ));
        }

        let x0_bytes: [u8; 33] = bytes[0..33].try_into().unwrap();
        let x1_bytes: [u8; 33] = bytes[33..66].try_into().unwrap();
        let x2_bytes: [u8; 33] = bytes[66..99].try_into().unwrap();

        let x0 = Option::<AffinePoint>::from(AffinePoint::from_bytes(&x0_bytes.into()))
            .map(ProjectivePoint::from)
            .ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid X0 point".to_string()))?;

        let x1 = Option::<AffinePoint>::from(AffinePoint::from_bytes(&x1_bytes.into()))
            .map(ProjectivePoint::from)
            .ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid X1 point".to_string()))?;

        let x2 = Option::<AffinePoint>::from(AffinePoint::from_bytes(&x2_bytes.into()))
            .map(ProjectivePoint::from)
            .ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid X2 point".to_string()))?;

        Ok(Self { x0, x1, x2 })
    }
}

/// Client Secrets for credential request
pub struct ClientSecrets {
    pub m1: Scalar, // Client attribute (e.g., user ID)
    pub s: Scalar,  // Blinding factor for Pedersen commitment
    pub r1: Scalar, // Randomness for auxiliary encryption
    pub r2: Scalar, // Randomness (unused in basic protocol, for extension)
}

impl ClientSecrets {
    /// Generate random client secrets
    /// Following CMZ14 MACGGM pattern
    pub fn random() -> Self {
        use rand_core::OsRng;
        Self {
            m1: Scalar::random(&mut OsRng),
            s: Scalar::random(&mut OsRng), // CMZ14 blinding factor
            r1: Scalar::random(&mut OsRng),
            r2: Scalar::random(&mut OsRng),
        }
    }
}

/// ARC Credential Request (from client to server)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ARCCredentialRequest {
    /// Blinded commitment
    pub m1_commit_blinded: Vec<u8>, // ProjectivePoint

    /// Encryption commitments
    pub x0_aux: Vec<u8>, // ProjectivePoint
    pub x1_aux: Vec<u8>, // ProjectivePoint
    pub x2_aux: Vec<u8>, // ProjectivePoint
}

impl ARCCredentialRequest {
    /// Create a credential request
    /// Following CMZ14 MACGGM blinded request (lox pattern line 220)
    /// CommitBlind = s*G + m1*X[1]
    pub fn new(
        client_secrets: &ClientSecrets,
        server_public_key: &ServerPublicKey,
        generators: &ARCGenerators,
    ) -> Self {
        // CMZ14 Pedersen commitment: CommitBlind = s*G + m1*X1
        // where s is the blinding factor and m1 is the client attribute
        let commit_blind =
            generators.g * client_secrets.s + server_public_key.x1 * client_secrets.m1;

        // Auxiliary point for encryption (will hold encrypted BlindQ)
        // Using a simple commitment for now
        let x0_aux = generators.g * client_secrets.r1;
        let x1_aux = generators.g * Scalar::ZERO;
        let x2_aux = generators.g * Scalar::ZERO;

        Self {
            m1_commit_blinded: commit_blind.to_affine().to_bytes().to_vec(),
            x0_aux: x0_aux.to_affine().to_bytes().to_vec(),
            x1_aux: x1_aux.to_affine().to_bytes().to_vec(),
            x2_aux: x2_aux.to_affine().to_bytes().to_vec(),
        }
    }

    /// Parse from bytes
    pub fn to_points(
        &self,
    ) -> Result<(
        ProjectivePoint,
        ProjectivePoint,
        ProjectivePoint,
        ProjectivePoint,
    )> {
        let m1_commit = Self::deserialize_point(&self.m1_commit_blinded)?;
        let x0_aux = Self::deserialize_point(&self.x0_aux)?;
        let x1_aux = Self::deserialize_point(&self.x1_aux)?;
        let x2_aux = Self::deserialize_point(&self.x2_aux)?;

        Ok((m1_commit, x0_aux, x1_aux, x2_aux))
    }

    fn deserialize_point(bytes: &[u8]) -> Result<ProjectivePoint> {
        if bytes.len() != 33 {
            return Err(ACDPError::ARCVerificationFailed(
                "Invalid point length".to_string(),
            ));
        }

        let arr: [u8; 33] = bytes.try_into().unwrap();
        Option::<AffinePoint>::from(AffinePoint::from_bytes(&arr.into()))
            .map(ProjectivePoint::from)
            .ok_or_else(|| ACDPError::ARCVerificationFailed("Invalid point encoding".to_string()))
    }
}

/// ARC Credential Response (from server to client)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ARCCredentialResponse {
    /// Unblinded U
    pub u: Vec<u8>, // ProjectivePoint

    /// Encrypted U'
    pub enc_u_prime: Vec<u8>, // ProjectivePoint

    /// Encryption helper points
    pub x0_aux: Vec<u8>, // ProjectivePoint
    pub x1_aux: Vec<u8>, // ProjectivePoint
    pub x2_aux: Vec<u8>, // ProjectivePoint

    /// ZK proof of correct issuance (stubbed for now)
    pub proof: Vec<u8>,
}

impl ARCCredentialResponse {
    /// Create credential response (server-side issuance)
    /// Following CMZ14 MACGGM blinded issuance protocol
    pub fn issue(
        request: &ARCCredentialRequest,
        server_private_key: &ServerPrivateKey,
        m2: Scalar, // Server-provided attribute (e.g., bucket ID)
        generators: &ARCGenerators,
    ) -> Result<Self> {
        use rand_core::OsRng;

        let (m1_commit_blinded, x0_aux, _x1_aux, _x2_aux) = request.to_points()?;

        // Server chooses random blinding factor b
        let b = Scalar::random(&mut OsRng);

        // CMZ14 MACGGM: P = b*G
        let p = generators.g * b;

        // CMZ14 MACGGM: BlindQ = b*CommitBlind + (x0 + m2*x2)*P
        // where CommitBlind = s*G + m1*X1 (from client)
        // Note: m1 is blinded in the commitment, m2 is revealed to server
        let blind_q =
            m1_commit_blinded * b + p * (server_private_key.x0 + m2 * server_private_key.x2);

        // Encrypt BlindQ with auxiliary point (so client can decrypt)
        let enc_u_prime = blind_q + x0_aux;

        // ZK proof (stubbed - will implement with sigma-proofs)
        let proof = vec![0u8; 64]; // Placeholder

        Ok(Self {
            u: p.to_affine().to_bytes().to_vec(), // Send P to client
            enc_u_prime: enc_u_prime.to_affine().to_bytes().to_vec(),
            x0_aux: request.x0_aux.clone(),
            x1_aux: request.x1_aux.clone(),
            x2_aux: request.x2_aux.clone(),
            proof,
        })
    }
}

/// ARC Credential (client holds this after receiving response)
#[derive(Debug, Serialize, Deserialize)]
pub struct ARCCredential {
    #[serde(with = "scalar_serde")]
    pub m1: Scalar,

    #[serde(with = "point_serde")]
    pub u: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub u_prime: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub x1: ProjectivePoint, // Server public key X1

    pub max_presentations: u64,

    #[serde(skip)]
    pub presentations_used: AtomicU64,
}

impl Clone for ARCCredential {
    fn clone(&self) -> Self {
        Self {
            m1: self.m1,
            u: self.u,
            u_prime: self.u_prime,
            x1: self.x1,
            max_presentations: self.max_presentations,
            presentations_used: AtomicU64::new(self.presentations_used.load(Ordering::SeqCst)),
        }
    }
}

impl ARCCredential {
    /// Finalize credential from server response
    /// Following CMZ14 MACGGM client-side unblinding
    pub fn from_response(
        response: &ARCCredentialResponse,
        _request: &ARCCredentialRequest,
        client_secrets: &ClientSecrets,
        server_public_key: &ServerPublicKey,
    ) -> Result<Self> {
        use rand_core::OsRng;

        // Parse points from response
        let p = ARCCredentialRequest::deserialize_point(&response.u)?; // Server's P = b*G
        let enc_blind_q = ARCCredentialRequest::deserialize_point(&response.enc_u_prime)?;
        let x0_aux = ARCCredentialRequest::deserialize_point(&response.x0_aux)?;

        // Verify proof (stubbed for now)
        // TODO: Implement ZK proof verification

        // Decrypt BlindQ: BlindQ = enc(BlindQ) - X0Aux
        let blind_q = enc_blind_q - x0_aux;

        // CMZ14 unblinding (lox pattern line 509):
        // Choose random r for rerandomization
        let r = Scalar::random(&mut OsRng);

        // Unblind: Q = r * (BlindQ - s*P)
        let q = (blind_q - p * client_secrets.s) * r;

        // Rerandomize P: U = r * P
        let u = p * r;

        // Final credential has MAC (U, Q) where Q = (x0 + m1*x1 + m2*x2)*U
        Ok(Self {
            m1: client_secrets.m1,
            u,          // P component of MAC
            u_prime: q, // Q component of MAC
            x1: server_public_key.x1,
            max_presentations: 1000, // Will be parameterized
            presentations_used: AtomicU64::new(0),
        })
    }

    /// Create unlinkable presentation
    pub fn create_presentation(
        &self,
        presentation_context: &[u8],
        nonce: u64,
        generators: &ARCGenerators,
    ) -> Result<ARCPresentation> {
        let used = self.presentations_used.load(Ordering::SeqCst);
        if used >= self.max_presentations {
            return Err(ACDPError::RateLimitExceeded {
                used,
                max: self.max_presentations,
            });
        }

        use rand_core::OsRng;

        // Randomize (U, U')
        let a = Scalar::random(&mut OsRng);
        let u_rand = self.u * a;
        let u_prime_rand = self.u_prime * a;

        // Random blinding factors
        let r = Scalar::random(&mut OsRng);
        let z = Scalar::random(&mut OsRng);

        // m1Commit = m1 * U + z * G (use G for consistency with MAC)
        let m1_commit = u_rand * self.m1 + generators.g * z;

        // U'Commit = U' + r * G
        let u_prime_commit = u_prime_rand + generators.g * r;

        // V = z * X1 - r * G (helper for ZK proof, where X1 = x1*G)
        let v = self.x1 * z - generators.g * r;

        // Compute tag: (m1 + nonce)^(-1) * H2G(presentationContext)
        let nonce_scalar = Scalar::from(nonce);
        let inverse = (self.m1 + nonce_scalar).invert();

        if inverse.is_none().into() {
            return Err(ACDPError::ARCVerificationFailed(
                "Cannot invert (m1 + nonce)".to_string(),
            ));
        }
        let inverse = inverse.unwrap();

        let dst = b"HashToGroup-ARCV1-P256Tag";
        let t = NistP256::hash_from_bytes::<ExpandMsgXmd<Sha256>>(&[presentation_context], &[dst])
            .expect("hash-to-curve should not fail");

        // tag = T / (m1 + nonce), where T = H2G(context)
        let tag = t * inverse;

        // m1Tag = m1 * tag (for ZK proof constraint)
        let m1_tag = tag * self.m1;

        // Generate ZK proof using sigma-proofs
        use crate::arc_zkp::ARCPresentationProof;

        let zkp = ARCPresentationProof::create(
            &self.m1,
            &z,
            &r,
            &nonce_scalar,
            &u_rand,
            &self.x1,
            &generators.g,
            &generators.h,
            &tag,
            &m1_commit,
            &v,
            &m1_tag,
            presentation_context,
        )?;

        let proof = ARCProof {
            proof: zkp.to_bytes().to_vec(),
        };

        // Increment counter
        self.presentations_used.fetch_add(1, Ordering::SeqCst);

        Ok(ARCPresentation {
            u: u_rand,
            u_prime_commit,
            m1_commit,
            tag,
            m1_tag,
            t,
            proof,
        })
    }

    /// Get presentations remaining
    pub fn presentations_remaining(&self) -> u64 {
        let used = self.presentations_used.load(Ordering::SeqCst);
        self.max_presentations.saturating_sub(used)
    }
}

/// ARC Presentation (sent to verifier)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ARCPresentation {
    #[serde(with = "point_serde")]
    pub u: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub u_prime_commit: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub m1_commit: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub tag: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub m1_tag: ProjectivePoint,

    #[serde(with = "point_serde")]
    pub t: ProjectivePoint,

    pub proof: ARCProof,
}

impl ARCPresentation {
    /// Verify presentation (server-side)
    pub fn verify(
        &self,
        server_private_key: &ServerPrivateKey,
        m2: Scalar,
        presentation_context: &[u8],
        nonce: u64,
        presentation_limit: u64,
        generators: &ARCGenerators,
    ) -> Result<bool> {
        // Check nonce in valid range
        if nonce >= presentation_limit {
            return Ok(false);
        }

        // Check U and U'Commit are not identity (constant-time)
        let u_is_identity = self.u.to_affine().is_identity();
        let u_prime_is_identity = self.u_prime_commit.to_affine().is_identity();

        if bool::from(u_is_identity | u_prime_is_identity) {
            return Ok(false);
        }

        // Verify the CMZ14 MAC structure
        // The credential has MAC (U, Q) where Q = (x0 + m1*x1 + m2*x2)*U
        // In presentation, both U and Q are randomized by factor 'a'
        // u_prime_commit = a*Q + r*G = (x0 + m1*x1 + m2*x2)*u_rand + r*G
        // m1_commit = m1*u_rand + z*G
        //
        // To verify the MAC relation, we compute:
        // V_raw = u_prime_commit - x0*u_rand - x1*m1_commit - m2*x2*u_rand
        //       = r*G - z*G*x1
        //
        // The ZK proof uses: V = z*X1 - r*G = z*(x1*G) - r*G
        // We need to match signs: V = -(r*G - z*G*x1) = z*G*x1 - r*G
        let v = self.m1_commit * server_private_key.x1
            + self.u * server_private_key.x0
            + self.u * (server_private_key.x2 * m2)
            - self.u_prime_commit;

        // Verify ZK proof using sigma-proofs
        use crate::arc_zkp::ARCPresentationProof;

        let zkp = ARCPresentationProof::from_bytes(self.proof.proof.clone());

        // Compute X1 from server private key
        let x1 = generators.g * server_private_key.x1;

        // Use the t and m1_tag from the presentation (they're part of the ZK proof statement)
        zkp.verify(
            &self.u,
            &x1,
            &generators.g,
            &generators.h,
            &self.tag,
            &self.m1_commit,
            &v,
            &self.m1_tag,
            &self.t,
            presentation_context,
        )?;

        // Verify t and m1_tag are correctly formed
        let dst = b"HashToGroup-ARCV1-P256Tag";
        let expected_t =
            NistP256::hash_from_bytes::<ExpandMsgXmd<Sha256>>(&[presentation_context], &[dst])
                .expect("hash-to-curve should not fail");

        let nonce_scalar = Scalar::from(nonce);
        let expected_m1_tag = expected_t - self.tag * nonce_scalar;

        // Check t and m1_tag match expectations
        if self.t != expected_t || self.m1_tag != expected_m1_tag {
            return Ok(false);
        }

        Ok(true)
    }
}

/// ARC Zero-Knowledge Proof
///
/// Wraps the sigma-proofs implementation for ARC credential presentations
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ARCProof {
    pub proof: Vec<u8>,
}

/// Serde module for Scalar
mod scalar_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(scalar: &Scalar, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = scalar.to_bytes();
        serializer.serialize_bytes(&bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Scalar, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid scalar length"));
        }

        let arr: [u8; 32] = bytes.try_into().unwrap();
        let field_bytes = FieldBytes::<NistP256>::from(arr);
        let scalar_opt: Option<Scalar> = Scalar::from_repr(field_bytes).into();
        scalar_opt.ok_or_else(|| serde::de::Error::custom("Invalid scalar encoding"))
    }
}

/// Serde module for ProjectivePoint
mod point_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        point: &ProjectivePoint,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = point.to_affine().to_bytes();
        serializer.serialize_bytes(&bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<ProjectivePoint, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        if bytes.len() != 33 {
            return Err(serde::de::Error::custom("Invalid point length"));
        }

        let arr: [u8; 33] = bytes.try_into().unwrap();
        Option::<AffinePoint>::from(AffinePoint::from_bytes(&arr.into()))
            .map(ProjectivePoint::from)
            .ok_or_else(|| serde::de::Error::custom("Invalid point encoding"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generators() {
        let gen = ARCGenerators::new();

        // G should be the standard generator
        assert_eq!(gen.g, ProjectivePoint::GENERATOR);

        // H should be different from G
        assert_ne!(gen.h, gen.g);

        // H should be deterministic
        let gen2 = ARCGenerators::new();
        assert_eq!(gen.h, gen2.h);
    }

    #[test]
    fn test_keypair() {
        let gen = ARCGenerators::new();
        let sk = ServerPrivateKey::random();
        let pk = ServerPublicKey::from_private_key(&sk, &gen);

        // Verify CMZ14 MACGGM public key derivation:
        // X[0] = x0_blinding*G + x0*H  (dual generator)
        // X[i] = x[i]*G for i > 0
        assert_eq!(pk.x0, gen.g * sk.x0_blinding + gen.h * sk.x0);
        assert_eq!(pk.x1, gen.g * sk.x1);
        assert_eq!(pk.x2, gen.g * sk.x2);
    }

    #[test]
    fn test_request_response_flow() {
        let gen = ARCGenerators::new();
        let sk = ServerPrivateKey::random();
        let pk = ServerPublicKey::from_private_key(&sk, &gen);

        // Client creates request
        let client_secrets = ClientSecrets::random();
        let request = ARCCredentialRequest::new(&client_secrets, &pk, &gen);

        // Server issues response
        let m2 = Scalar::from(0u64);
        let response = ARCCredentialResponse::issue(&request, &sk, m2, &gen).unwrap();

        // Client finalizes credential
        let credential =
            ARCCredential::from_response(&response, &request, &client_secrets, &pk).unwrap();

        assert_eq!(credential.presentations_remaining(), 1000);
    }

    #[test]
    fn test_presentation() {
        use rand_core::OsRng;

        let gen = ARCGenerators::new();
        let sk = ServerPrivateKey::random();
        let pk = ServerPublicKey::from_private_key(&sk, &gen);

        let client_secrets = ClientSecrets::random();
        let request = ARCCredentialRequest::new(&client_secrets, &pk, &gen);
        let m2 = Scalar::from(0u64);
        let response = ARCCredentialResponse::issue(&request, &sk, m2, &gen).unwrap();
        let credential =
            ARCCredential::from_response(&response, &request, &client_secrets, &pk).unwrap();

        // Create presentation
        let context = b"test-context";
        let nonce = 42;

        // Manually compute what the presentation should contain for debugging
        let a = Scalar::random(&mut OsRng);
        let u_rand = credential.u * a;
        let q_rand = credential.u_prime * a;

        // Verify the MAC before presentation
        let expected_q = u_rand * (sk.x0 + credential.m1 * sk.x1 + m2 * sk.x2);
        eprintln!(
            "q_rand == expected_q: {}",
            q_rand.to_affine() == expected_q.to_affine()
        );

        let presentation = credential
            .create_presentation(context, nonce, &gen)
            .unwrap();

        // Verify presentation
        let valid = presentation
            .verify(&sk, m2, context, nonce, 1000, &gen)
            .unwrap();
        assert!(valid);

        assert_eq!(credential.presentations_remaining(), 999);
    }

    #[test]
    fn test_serialization() {
        let gen = ARCGenerators::new();
        let sk = ServerPrivateKey::random();
        let pk = ServerPublicKey::from_private_key(&sk, &gen);

        // Test server key serialization
        let sk_bytes = sk.to_bytes();
        let sk2 = ServerPrivateKey::from_bytes(&sk_bytes).unwrap();
        assert_eq!(sk.x0.to_bytes(), sk2.x0.to_bytes());

        // Test public key serialization
        let pk_bytes = pk.to_bytes();
        let pk2 = ServerPublicKey::from_bytes(&pk_bytes).unwrap();
        assert_eq!(pk.x0, pk2.x0);
        assert_eq!(pk.x1, pk2.x1);
        assert_eq!(pk.x2, pk2.x2);
    }
}
