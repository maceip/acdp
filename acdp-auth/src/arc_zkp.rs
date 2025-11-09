//! Zero-Knowledge Proofs for ARC using sigma-proofs
//!
//! Implements the 4 ZK proof constraints for ARC credential presentations:
//! 1. m1Commit = m1 * U + z * G (commitment to m1)
//! 2. V = z * X1 - r * G (commitment randomness)
//! 3. T = (m1 + nonce) * tag (tag correctness)
//! 4. m1Tag = m1 * tag (nonce consistency)

use crate::error::{ACDPError, Result};
use p256::{ProjectivePoint, Scalar};
use sigma_proofs::LinearRelation;

/// ARC Presentation Proof
///
/// Proves knowledge of (m1, z, r, nonce) such that the 4 ARC constraints hold
pub struct ARCPresentationProof {
    /// Non-interactive zero-knowledge proof
    pub proof: Vec<u8>,
}

impl ARCPresentationProof {
    /// Create a proof for ARC credential presentation
    ///
    /// # Arguments
    /// * `m1` - Client attribute (scalar)
    /// * `z` - Randomness for U' (scalar)
    /// * `r` - Randomness for V (scalar)
    /// * `nonce` - Presentation nonce (scalar)
    /// * `u` - Randomized credential point U
    /// * `x1` - Server public key component X1
    /// * `g` - Generator G
    /// * `h` - Generator H
    /// * `tag` - Presentation tag point
    /// * `m1_commit` - Commitment to m1
    /// * `v` - Commitment V
    ///
    /// # Returns
    /// Serialized non-interactive proof
    pub fn create(
        m1: &Scalar,
        z: &Scalar,
        r: &Scalar,
        nonce: &Scalar,
        u: &ProjectivePoint,
        x1: &ProjectivePoint,
        g: &ProjectivePoint,
        h: &ProjectivePoint,
        tag: &ProjectivePoint,
        m1_commit: &ProjectivePoint,
        v: &ProjectivePoint,
        m1_tag: &ProjectivePoint,
        presentation_context: &[u8],
    ) -> Result<Self> {
        use rand_core::OsRng;
        let mut rng = OsRng;

        // Create linear relation for all 4 constraints
        let mut relation = LinearRelation::<ProjectivePoint>::new();

        // Allocate scalar variables (witnesses)
        let [m1_var, z_var, r_var, nonce_var] = relation.allocate_scalars();

        // Allocate element variables (public inputs)
        let [u_var, x1_var, g_var, h_var, tag_var] = relation.allocate_elements();

        // Constraint 1: m1Commit = m1 * U + z * G
        let m1_commit_var = relation.allocate_eq(m1_var * u_var + z_var * g_var);

        // Constraint 2: V = z * X1 - r * G
        let v_var = relation.allocate_eq(z_var * x1_var - r_var * g_var);

        // Constraint 3: T = (m1 + nonce) * tag
        // This is: T = m1 * tag + nonce * tag
        let t_var = relation.allocate_eq(m1_var * tag_var + nonce_var * tag_var);

        // Constraint 4: m1Tag = m1 * tag
        let m1_tag_var = relation.allocate_eq(m1_var * tag_var);

        // Set public elements
        relation.set_element(u_var, *u);
        relation.set_element(x1_var, *x1);
        relation.set_element(g_var, *g);
        relation.set_element(h_var, *h);
        relation.set_element(tag_var, *tag);
        relation.set_element(m1_commit_var, *m1_commit);
        relation.set_element(v_var, *v);
        relation.set_element(m1_tag_var, *m1_tag);

        // Compute T = (m1 + nonce) * tag for constraint 3
        let m1_plus_nonce = *m1 + nonce;
        let t_value = *tag * m1_plus_nonce;
        relation.set_element(t_var, t_value);

        // Convert to non-interactive proof using Fiat-Shamir
        let session_id = format!(
            "ARC-P256-presentation:{}",
            hex::encode(presentation_context)
        );
        let nizk = relation.into_nizk(session_id.as_bytes()).map_err(|e| {
            ACDPError::ARCVerificationFailed(format!("NIZK creation failed: {:?}", e))
        })?;

        // Create witness vector
        let witness = vec![*m1, *z, *r, *nonce];

        // Generate proof
        let proof = nizk.prove_batchable(&witness, &mut rng).map_err(|e| {
            ACDPError::ARCVerificationFailed(format!("Proof generation failed: {:?}", e))
        })?;

        Ok(Self { proof })
    }

    /// Verify an ARC presentation proof
    ///
    /// # Arguments
    /// Public inputs matching those used in proof creation
    ///
    /// # Returns
    /// Ok(()) if proof is valid, error otherwise
    pub fn verify(
        &self,
        u: &ProjectivePoint,
        x1: &ProjectivePoint,
        g: &ProjectivePoint,
        h: &ProjectivePoint,
        tag: &ProjectivePoint,
        m1_commit: &ProjectivePoint,
        v: &ProjectivePoint,
        m1_tag: &ProjectivePoint,
        t: &ProjectivePoint,
        presentation_context: &[u8],
    ) -> Result<()> {
        // Recreate linear relation with same structure
        let mut relation = LinearRelation::<ProjectivePoint>::new();

        let [m1_var, z_var, r_var, nonce_var] = relation.allocate_scalars();
        let [u_var, x1_var, g_var, h_var, tag_var] = relation.allocate_elements();

        let m1_commit_var = relation.allocate_eq(m1_var * u_var + z_var * g_var);
        let v_var = relation.allocate_eq(z_var * x1_var - r_var * g_var);
        let t_var = relation.allocate_eq(m1_var * tag_var + nonce_var * tag_var);
        let m1_tag_var = relation.allocate_eq(m1_var * tag_var);

        // Set public elements
        relation.set_element(u_var, *u);
        relation.set_element(x1_var, *x1);
        relation.set_element(g_var, *g);
        relation.set_element(h_var, *h);
        relation.set_element(tag_var, *tag);
        relation.set_element(m1_commit_var, *m1_commit);
        relation.set_element(v_var, *v);
        relation.set_element(m1_tag_var, *m1_tag);
        relation.set_element(t_var, *t);

        // Convert to NIZK
        let session_id = format!(
            "ARC-P256-presentation:{}",
            hex::encode(presentation_context)
        );
        let nizk = relation.into_nizk(session_id.as_bytes()).map_err(|e| {
            ACDPError::ARCVerificationFailed(format!("NIZK creation failed: {:?}", e))
        })?;

        // Verify proof
        nizk.verify_batchable(&self.proof).map_err(|e| {
            ACDPError::ARCVerificationFailed(format!("Proof verification failed: {:?}", e))
        })?;

        Ok(())
    }

    /// Serialize proof to bytes
    pub fn to_bytes(&self) -> &[u8] {
        &self.proof
    }

    /// Deserialize proof from bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { proof: bytes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff::Field;

    #[test]
    fn test_arc_presentation_proof() {
        use rand_core::OsRng;

        // Setup generators
        let g = ProjectivePoint::GENERATOR;
        let h = ProjectivePoint::GENERATOR * Scalar::from(2u64);

        // Create witness values
        let m1 = Scalar::random(&mut OsRng);
        let z = Scalar::random(&mut OsRng);
        let r = Scalar::random(&mut OsRng);
        let nonce = Scalar::from(42u64);

        // Create public values
        let x1 = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);
        let u = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);
        let tag = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);

        // Compute commitments (using G for consistency)
        let m1_commit = u * m1 + g * z;
        let v = x1 * z - g * r;
        let m1_tag = tag * m1;
        let t = tag * (m1 + nonce);

        let context = b"test-presentation";

        // Create proof
        let proof = ARCPresentationProof::create(
            &m1, &z, &r, &nonce, &u, &x1, &g, &h, &tag, &m1_commit, &v, &m1_tag, context,
        )
        .expect("proof creation failed");

        // Verify proof
        proof
            .verify(&u, &x1, &g, &h, &tag, &m1_commit, &v, &m1_tag, &t, context)
            .expect("proof verification failed");
    }

    #[test]
    fn test_invalid_proof_fails() {
        use rand_core::OsRng;

        let g = ProjectivePoint::GENERATOR;
        let h = ProjectivePoint::GENERATOR * Scalar::from(2u64);

        let m1 = Scalar::random(&mut OsRng);
        let z = Scalar::random(&mut OsRng);
        let r = Scalar::random(&mut OsRng);
        let nonce = Scalar::from(42u64);

        let x1 = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);
        let u = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);
        let tag = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);

        let m1_commit = u * m1 + g * z;
        let v = x1 * z - g * r;
        let m1_tag = tag * m1;
        let t = tag * (m1 + nonce);

        let context = b"test-presentation";

        let proof = ARCPresentationProof::create(
            &m1, &z, &r, &nonce, &u, &x1, &g, &h, &tag, &m1_commit, &v, &m1_tag, context,
        )
        .expect("proof creation failed");

        // Verify with wrong tag should fail
        let wrong_tag = ProjectivePoint::GENERATOR * Scalar::random(&mut OsRng);
        let result = proof.verify(
            &u, &x1, &g, &h, &wrong_tag, &m1_commit, &v, &m1_tag, &t, context,
        );

        assert!(result.is_err());
    }
}
