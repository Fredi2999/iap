use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{error::VaultError, meta::VaultMeta};

/// Löscht das abgeleitete Schlüsselmaterial beim Drop bestmöglich aus dem RAM.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey([u8; 32]);

impl VaultKey {
    /// Ist die eng benannte Übergabegrenze zur SQLCipher-Rohschlüsselschnittstelle.
    pub fn expose_for_sqlcipher(&self) -> &[u8; 32] {
        &self.0
    }
}

impl VaultKey {
    /// Vergleicht in konstanter Zeit, damit ein Passwortvergleich keine Laufzeit verrät.
    pub fn matches(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
    }

    /// Eine zweite Kopie für den Rückweg beim Schlüsselwechsel. Beide Kopien werden beim Drop gelöscht.
    pub(crate) fn duplicate(&self) -> Self {
        Self(self.0)
    }
}

/// Leitet einen 256-Bit-Rohschlüssel mit den im Vault fixierten Argon2id-v19-Kosten ab.
pub fn derive_key(passphrase: &str, meta: &VaultMeta) -> Result<VaultKey, VaultError> {
    let parameters = Params::new(
        meta.parameters.memory_kib,
        meta.parameters.iterations,
        meta.parameters.parallelism,
        Some(32),
    )
    .map_err(|error| VaultError::KeyDerivation(error.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters);
    let mut output = [0_u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), &meta.salt()?, &mut output)
        .map_err(|error| VaultError::KeyDerivation(error.to_string()))?;
    Ok(VaultKey(output))
}
