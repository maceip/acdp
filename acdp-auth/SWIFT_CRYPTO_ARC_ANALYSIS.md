# Swift Crypto ARC Implementation Analysis

## Overview

Swift Crypto (`/Users/rpm/swift-crypto/Sources/CryptoExtras/ARC/`) contains Apple's production-quality implementation of Anonymous Rate-Limited Credentials (ARC) based on the [IETF draft-yun-cfrg-arc](https://datatracker.ietf.org/doc/draft-yun-cfrg-arc).

## Implementation Summary

### Architecture

**Protocol**: CMZ14 MACGGM (Message Authentication Code - Generalized Groth-Maller)
- Uses elliptic curve cryptography (P-256, P-384)
- Zero-knowledge proofs for unlinkable presentations
- Rate limiting via nonce tracking

### Key Components

1. **`ARC.swift`** (75 lines)
   - Ciphersuite definitions (P-256, P-384)
   - Generator computation (G, H)
   - Suite IDs and domain separation strings

2. **`ARCCredential.swift`** (116 lines)
   - Credential structure: `(m1, U, U', X1)`
   - Presentation state management
   - Nonce tracking and collision handling

3. **`ARCPresentation.swift`** (153 lines)
   - Zero-knowledge proof generation
   - Presentation verification
   - Proof constraints (4 key statements)

4. **`ARC+API.swift`** (802 lines total)
   - Public API for P-256/P-384
   - Server private/public keys
   - Request/response handling

5. **`ARCServer.swift`** (129 lines)
   - Server-side credential issuance
   - Proof verification

## Complexity Assessment

### Easy to Port ✅

1. **Data structures** (90% straightforward)
   - Credential, Request, Response, Presentation
   - Nonce state management
   - Serialization/deserialization

2. **Core protocol flow**
   - Request → Response → Credential → Presentation
   - Clear separation of client/server logic

### Moderate Difficulty ⚠️

1. **Elliptic Curve Operations** (depends on existing Rust libraries)
   - Scalar multiplication: `a * U`
   - Point addition: `U + V`
   - Scalar inversion: `(m1 + nonce)^(-1)`
   - Hash-to-curve: `H2G(data, DST)`

   **Rust equivalent**: Use `p256` or `p384` crates
   ```rust
   use p256::{
       elliptic_curve::{Group, ScalarPrimitive},
       AffinePoint, ProjectivePoint, Scalar
   };
   ```

2. **Hash-to-Curve**
   - Swift uses `HashToCurveImpl<P256>`
   - Rust: `p256::elliptic_curve::hash2curve` (RFC 9380)

### Complex/Critical 🔴

1. **Zero-Knowledge Proof System** (most complex part)

   Swift uses custom `Prover`/`Verifier` with constraints:
   ```swift
   var prover = Prover<H2G>(label: "CredentialPresentation")
   let m1Var = prover.appendScalar(label: "m1", assignment: m1)
   prover.constrain(result: m1CommitVar, linearCombination: [(m1Var, UVar), (zVar, genHVar)])
   let proof = try prover.prove()
   ```

   **Proof statements**:
   1. `m1Commit = m1 * U + z * generatorH` (commitment to m1)
   2. `V = z * X1 - r * generatorG` (commitment randomness)
   3. `H2G(ctx) = m1 * tag + nonce * tag` (tag correctness)
   4. `m1Tag = m1 * tag` (nonce consistency)

   **Rust options**:
   - **Option A**: Use `bulletproofs` crate (Range proofs, R1CS)
   - **Option B**: Use `ark-groth16` (zkSNARKs, may be overkill)
   - **Option C**: Implement Schnorr Σ-protocols directly
     - More aligned with ARC spec
     - ~300-500 lines of careful crypto code

2. **Constant-Time Operations**
   - Swift relies on BoringSSL for side-channel resistance
   - Rust: Must use `subtle` crate for constant-time comparisons
   - Critical for: scalar ops, point equality, nonce checks

## Dependencies

### Swift Crypto Uses

```
BoringSSL (vendored)
├── Elliptic curve ops (P-256, P-384)
├── SHA-256/SHA-384
├── HKDF
└── Constant-time operations

Foundation
├── Data types
└── Serialization
```

### Rust Equivalents

```toml
[dependencies]
# Core elliptic curve support
p256 = { version = "0.13", features = ["hash2curve", "arithmetic"] }
p384 = { version = "0.13", features = ["hash2curve", "arithmetic"] }

# Cryptographic primitives
sha2 = "0.10"
hkdf = "0.12"
subtle = "2.5"  # Constant-time ops

# Serialization
serde = { version = "1.0", features = ["derive"] }
hex = "0.4"

# Zero-knowledge proofs (choose one)
# Option 1: Schnorr protocols (cleanroom impl)
# Option 2: Bulletproofs
bulletproofs = "4.0"
# Option 3: ark-groth16 (overkill but available)
ark-groth16 = "0.4"
```

## Cleanroom Reimplementation Effort

### Phase 1: Data Structures (1-2 days)
- [x] Basic types: Scalar, Point, Credential, Presentation
- [x] Serialization/deserialization
- [x] Nonce state management

**Estimated lines**: ~500 (already done in our placeholder)

### Phase 2: Elliptic Curve Ops (2-3 days)
- [ ] Integrate `p256` crate
- [ ] Implement hash-to-curve (RFC 9380)
- [ ] Generator computation
- [ ] Scalar/point arithmetic wrappers

**Estimated lines**: ~300

**Risk**: Low (well-supported crates exist)

### Phase 3: ZK Proof System (5-7 days) ⚠️
This is the **critical path** and requires careful cryptographic implementation.

**Option A: Schnorr Σ-Protocols** (recommended)
```rust
// Prover generates commitments
let r1 = Scalar::random();
let A1 = r1 * G;

// Fiat-Shamir challenge
let c = hash(A1, A2, ..., public_inputs);

// Prover responses
let z1 = r1 + c * m1;

// Verifier checks
assert_eq!(z1 * G, A1 + c * m1Commit);
```

**Estimated lines**: ~400-600

**Challenges**:
- Fiat-Shamir transcript (use `merlin` crate)
- Batch verification optimizations
- Constant-time operations

**Option B: Use Bulletproofs** (if suitable)
- More complex than needed
- Well-tested library
- May not map cleanly to ARC's 4 constraints

**Estimated lines**: ~200-300 (if Bulletproofs fits)

### Phase 4: Server/Client API (2-3 days)
- [ ] Request/response handling
- [ ] Server credential issuance
- [ ] Client presentation generation
- [ ] Verification logic

**Estimated lines**: ~400

### Phase 5: Testing & Validation (3-5 days)
- [ ] Test vectors from IETF draft
- [ ] Fuzzing (especially proof verification)
- [ ] Side-channel testing (constant-time validation)
- [ ] Interop with Swift implementation

**Critical**: Must validate against Swift Crypto test vectors

## Total Effort Estimate

| Component | Lines of Code | Time | Risk |
|-----------|--------------|------|------|
| Data Structures | ~500 | 1-2 days | Low ✅ |
| EC Operations | ~300 | 2-3 days | Low ✅ |
| ZK Proofs | ~400-600 | 5-7 days | **High** 🔴 |
| API Layer | ~400 | 2-3 days | Low ✅ |
| Testing | - | 3-5 days | Medium ⚠️ |
| **TOTAL** | **~1600-1800** | **13-20 days** | |

## Cross-Compilation Feasibility

### Swift → Rust FFI (Not Recommended)

**Challenges**:
1. Swift calling convention differs from C ABI
2. Swift Crypto uses internal CoreCrypto on Apple platforms
3. BoringSSL vendoring would need Rust bindings
4. No clear benefit over cleanroom Rust impl

**Verdict**: ❌ Not worth the effort

### Cleanroom Rust Implementation (Recommended)

**Advantages**:
1. Pure Rust = no cross-compilation issues
2. Can use well-tested crates (`p256`, `subtle`, `merlin`)
3. Better integration with mcp-auth
4. No Swift runtime dependency
5. Easier to audit and test

**Disadvantages**:
1. Must reimplement ZK proof system carefully
2. Need to validate against Swift test vectors
3. ~2-3 weeks of careful cryptographic work

**Verdict**: ✅ Feasible but requires crypto expertise

## Recommended Approach

### Immediate (This Week)
1. ✅ Keep placeholder in mcp-auth (already done)
2. ✅ Focus on other ACDP components (credentials, delegation, gateway)
3. [ ] Extract test vectors from Swift Crypto for validation later

### Short Term (Next 2-3 Weeks)
1. [ ] Implement Phase 1-2 (data structures + EC ops)
2. [ ] Get basic request/response flow working
3. [ ] Stub out ZK proofs with placeholder commitments

### Medium Term (1-2 Months)
1. [ ] Implement Schnorr Σ-protocol proof system
2. [ ] Validate against Swift Crypto test vectors
3. [ ] Security audit of constant-time operations
4. [ ] Fuzzing and side-channel testing

## Key Risks

### Critical 🔴
- **ZK Proof Correctness**: Subtle bugs in proof generation/verification = broken security
- **Side-Channel Leaks**: Non-constant-time ops = timing attacks reveal secrets

### Medium ⚠️
- **Hash-to-Curve**: Must match RFC 9380 exactly for interoperability
- **Serialization**: Must match IETF draft format for cross-implementation compat

### Low ✅
- **EC Arithmetic**: Well-tested `p256` crate handles this
- **Nonce Tracking**: Straightforward state management

## Recommendation

**For mcp-auth v0.1**: Keep placeholders, focus on protocol integration

**For mcp-auth v1.0 (production)**:
- Option 1: **Cleanroom Rust impl** (2-3 weeks, high quality)
- Option 2: **Wait for Rust Privacy Pass crate** (if one emerges)
- Option 3: **Partner with Swift Crypto team** (get test vectors, coordinate)

**My vote**: Cleanroom Rust implementation with rigorous testing.

The Swift Crypto code is clean, well-documented, and follows the IETF draft closely. A careful port to Rust using `p256` + custom Schnorr proofs is very doable, but requires dedicated cryptographic expertise and testing.

## Next Steps

1. Extract Swift Crypto test vectors → `tests/arc_test_vectors.json`
2. Implement Phase 1-2 in `mcp-auth/src/arc.rs`
3. Write stub proof system that passes basic flow tests
4. Incrementally replace stubs with real crypto
5. Validate against Swift test vectors before marking stable

Would you like me to start on Phase 1-2 (data structures + EC ops)?
