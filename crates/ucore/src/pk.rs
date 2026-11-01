//! 32-byte public keys, 64-byte signatures, base58, PDA / ATA derivation.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Pk(pub [u8; 32]);

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

const fn b58_digit(c: u8) -> u32 {
    let mut i = 0;
    while i < 58 {
        if ALPHABET[i] == c {
            return i as u32;
        }
        i += 1;
    }
    panic!("invalid base58 character")
}

/// Compile-time base58 → 32 bytes. Panics (at compile time when used in a const) on bad input.
pub const fn pk(s: &str) -> Pk {
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < b.len() {
        let mut carry = b58_digit(b[i]);
        let mut j = 32;
        while j > 0 {
            j -= 1;
            let x = out[j] as u32 * 58 + carry;
            out[j] = (x & 0xff) as u8;
            carry = x >> 8;
        }
        if carry != 0 {
            panic!("base58 value does not fit in 32 bytes");
        }
        i += 1;
    }
    Pk(out)
}

impl Pk {
    pub const ZERO: Pk = Pk([0u8; 32]);

    pub fn from_b58(s: &str) -> anyhow::Result<Pk> {
        let v = bs58::decode(s).into_vec()?;
        if v.len() != 32 {
            anyhow::bail!("pubkey {s} decodes to {} bytes", v.len());
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        Ok(Pk(a))
    }
    pub fn from_slice(s: &[u8]) -> Option<Pk> {
        if s.len() < 32 {
            return None;
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&s[..32]);
        Some(Pk(a))
    }
    pub fn to_b58(&self) -> String {
        bs58::encode(self.0).into_string()
    }
    pub fn short(&self) -> String {
        let s = self.to_b58();
        format!("{}..{}", &s[..4], &s[s.len() - 4..])
    }
    pub fn is_on_curve(&self) -> bool {
        curve25519_dalek::edwards::CompressedEdwardsY(self.0)
            .decompress()
            .is_some()
    }
    pub fn create_pda(seeds: &[&[u8]], program: &Pk) -> Option<Pk> {
        let mut h = Sha256::new();
        for s in seeds {
            h.update(s);
        }
        h.update(program.0);
        h.update(b"ProgramDerivedAddress");
        let out: [u8; 32] = h.finalize().into();
        let p = Pk(out);
        if p.is_on_curve() {
            None
        } else {
            Some(p)
        }
    }
    pub fn find_pda(seeds: &[&[u8]], program: &Pk) -> (Pk, u8) {
        let mut bump: u8 = 255;
        loop {
            let b = [bump];
            let mut v: Vec<&[u8]> = seeds.to_vec();
            v.push(&b);
            if let Some(p) = Pk::create_pda(&v, program) {
                return (p, bump);
            }
            if bump == 0 {
                panic!("no PDA bump found");
            }
            bump -= 1;
        }
    }
    /// Associated token account address.
    pub fn ata(owner: &Pk, mint: &Pk, token_program: &Pk) -> Pk {
        Pk::find_pda(
            &[&owner.0, &token_program.0, &mint.0],
            &crate::pump::ATA_PROGRAM,
        )
        .0
    }
}

impl fmt::Display for Pk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b58())
    }
}
impl fmt::Debug for Pk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b58())
    }
}
impl std::str::FromStr for Pk {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Pk::from_b58(s)
    }
}

/// Transaction signature (64 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig(pub [u8; 64]);

impl Default for Sig {
    fn default() -> Self {
        Sig([0u8; 64])
    }
}
impl Sig {
    pub fn from_b58(s: &str) -> anyhow::Result<Sig> {
        let v = bs58::decode(s).into_vec()?;
        if v.len() != 64 {
            anyhow::bail!("signature decodes to {} bytes", v.len());
        }
        let mut a = [0u8; 64];
        a.copy_from_slice(&v);
        Ok(Sig(a))
    }
    pub fn from_slice(s: &[u8]) -> Option<Sig> {
        if s.len() != 64 {
            return None;
        }
        let mut a = [0u8; 64];
        a.copy_from_slice(s);
        Some(Sig(a))
    }
    pub fn to_b58(&self) -> String {
        bs58::encode(self.0).into_string()
    }
    /// 8-byte prefix used as a cheap identity key.
    pub fn key(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().unwrap())
    }
}
impl fmt::Display for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b58())
    }
}
impl fmt::Debug for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b58())
    }
}
impl Serialize for Sig {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}
impl<'de> Deserialize<'de> for Sig {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Sig;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("64 bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Sig, E> {
                Sig::from_slice(v).ok_or_else(|| E::invalid_length(v.len(), &self))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Sig, E> {
                self.visit_bytes(&v)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Sig, A::Error> {
                let mut a = [0u8; 64];
                for (i, b) in a.iter_mut().enumerate() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(Sig(a))
            }
        }
        d.deserialize_bytes(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn const_b58_roundtrip() {
        let s = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
        assert_eq!(pk(s).to_b58(), s);
        assert_eq!(pk("11111111111111111111111111111111"), Pk::ZERO);
        assert_eq!(Pk::from_b58(s).unwrap(), pk(s));
    }
    #[test]
    fn pump_pdas_match_docs() {
        use crate::pump::*;
        // global and event authority are documented constants
        let (g, _) = Pk::find_pda(&[b"global"], &PUMP_PROGRAM);
        assert_eq!(g, GLOBAL);
        let (ea, _) = Pk::find_pda(&[b"__event_authority"], &PUMP_PROGRAM);
        assert_eq!(ea, EVENT_AUTHORITY);
        let (fc, _) = Pk::find_pda(&[b"fee_config", &PUMP_PROGRAM.0], &PUMP_FEES_PROGRAM);
        assert_eq!(fc, FEE_CONFIG);
    }
}
