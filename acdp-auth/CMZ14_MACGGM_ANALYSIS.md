# CMZ14 MACGGM Construction Analysis

Based on lox-library implementation (Ristretto255/curve25519)

## Key Components

### Server Private Key
```rust
struct IssuerPrivKey {
    x0tilde: Scalar,  // Blinding key
    x: Vec<Scalar>,   // x[0], x[1], ..., x[n] attribute keys
}
```

### Server Public Key
```rust
struct IssuerPubKey {
    X: Vec<RistrettoPoint>
}
// where X[0] = x0tilde*A + x[0]*B
//       X[i] = x[i]*A for i > 0
```

## CMZ14 MAC Structure

A credential consists of `(P, Q)` where:
- `P` is a random point (chosen by client, blinded from server)
- `Q = (x[0] + m1*x[1] + m2*x[2] + ... + mn*x[n]) * P`

### Verification
```rust
pub fn verify_lox(&self, cred: &cred::Lox) -> bool {
    if cred.P.is_identity() {
        return false;
    }
    
    let Q = (self.lox_priv.x[0]
        + cred.id * self.lox_priv.x[1]
        + cred.bucket * self.lox_priv.x[2]
        + cred.trust_level * self.lox_priv.x[3]
        + cred.level_since * self.lox_priv.x[4]
        + cred.invites_remaining * self.lox_priv.x[5]
        + cred.blockages * self.lox_priv.x[6])
        * cred.P;
    
    Q == cred.Q
}
```

## Blinded Issuance Protocol

### Client Side (Request)
1. Client chooses random `s, r` scalars
2. Client computes `P = r * G` (random point)
3. Client computes `CommitLoxBlind = s * P + m1*X[1] + m2*X[2] + ...` (Pedersen commitment)

### Server Side (Issue)
1. Server chooses random `b` (blinding factor)
2. Server adds its contribution: `CommitLoxSrv = CommitLoxBlind + server_id * X[1]`
3. Server computes:
   ```rust
   BlindQ = b * CommitLoxSrv + (x[0] + revealed_attr1*x[i] + ...) * P
   ```

### Client Side (Finalize)
1. Client unblinds: `Q = r * (BlindQ - s * P)`

## Mathematical Proof

Starting from server's `BlindQ`:
```
BlindQ = b * (s*P + m1*X[1] + ...) + (x[0] + revealed_attrs) * P
       = b*s*P + b*(m1*X[1] + ...) + (x[0] + revealed_attrs) * P
```

Client computes:
```
Q = r * (BlindQ - s*P)
  = r * (b*s*P + b*(m1*X[1] + ...) + (x[0] + revealed_attrs)*P - s*P)
  = r * ((b-1)*s*P + b*(m1*X[1] + ...) + (x[0] + revealed_attrs)*P)
```

Wait, this doesn't work out correctly. Let me re-read the code...

Actually, looking at line 509:
```rust
let Q = r * (BlindQ - state.s * P)
```

Where `state.s` is the blinding factor. But `BlindQ` includes `b * CommitLoxSrv`.

The trick is that the client's `CommitLoxBlind` already includes `s*P`, so when the server multiplies by `b`, it becomes `b*s*P`. Then the client subtracts `s*P` and divides by `r`.

Actually, I need to trace through more carefully. Let me check how `CommitLoxBlind` is computed...

