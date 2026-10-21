use anyhow::Result;
use tracing::debug;

/// Lobby crypto state for one connection.
///
/// The key derivation is a prototype: the "session key" is the static
/// `t9_common::SESSION_KEY`, not something negotiated with the client, and the
/// handshake transcript is recorded but not yet hashed.
pub struct LobbyCrypto {
    transcript: Vec<Vec<u8>>,
    key: Option<[u8; 16]>,
    response_id: [u8; 8],
}

impl LobbyCrypto {
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            key: None,
            response_id: [0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37],
        }
    }

    pub fn queue_packet_to_hash(&mut self, data: &[u8]) {
        self.transcript.push(data.to_vec());
    }

    pub fn derive_keys(&mut self) {
        let session_key = t9_common::SESSION_KEY;

        let mut key = [0u8; 16];
        key.copy_from_slice(&session_key[..16]);
        self.key = Some(key);
        self.response_id.copy_from_slice(&session_key[16..24]);

        debug!(
            "[crypto] Derived keys after {} handshake packet(s)",
            self.transcript.len()
        );
    }

    pub fn response_id(&self) -> [u8; 8] {
        self.response_id
    }

    fn key(&self) -> Result<[u8; 16]> {
        self.key
            .ok_or_else(|| anyhow::anyhow!("key not set — auth not completed"))
    }

    /// AES-128-CBC with zero padding up to the block size.
    pub fn decrypt_lobby_packet(&self, encrypted: &[u8], iv_seed: &[u8]) -> Result<Vec<u8>> {
        use aes::cipher::KeyIvInit;
        use cbc::cipher::BlockDecryptMut;

        let key = self.key()?;
        let iv: [u8; 16] = iv_seed
            .try_into()
            .map_err(|_| anyhow::anyhow!("IV must be 16 bytes"))?;

        let mut data = encrypted.to_vec();
        if data.len() % 16 != 0 {
            let padding = 16 - (data.len() % 16);
            data.extend(vec![0u8; padding]);
        }

        type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
        let decryptor = Aes128CbcDec::new(&key.into(), &iv.into());

        let decrypted = decryptor
            .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut data)
            .map_err(|e| anyhow::anyhow!("AES decryption failed: {}", e))?;

        Ok(decrypted.to_vec())
    }

    /// Inverse of [`Self::decrypt_lobby_packet`]. Encrypted replies are not
    /// sent yet because their framing is unknown; tests use this to build
    /// client packets.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn encrypt_lobby_packet(&self, plaintext: &[u8], iv_seed: &[u8]) -> Result<Vec<u8>> {
        use aes::cipher::KeyIvInit;
        use cbc::cipher::BlockEncryptMut;

        let key = self.key()?;
        let iv: [u8; 16] = iv_seed
            .try_into()
            .map_err(|_| anyhow::anyhow!("IV must be 16 bytes"))?;

        let mut buf = plaintext.to_vec();
        buf.resize(plaintext.len().div_ceil(16) * 16, 0);

        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let encryptor = Aes128CbcEnc::new(&key.into(), &iv.into());

        let len = buf.len();
        let encrypted = encryptor
            .encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf, len)
            .map_err(|e| anyhow::anyhow!("AES encryption failed: {}", e))?;

        Ok(encrypted.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_required_before_crypto() {
        let crypto = LobbyCrypto::new();
        assert!(crypto.decrypt_lobby_packet(&[0; 16], &[0; 16]).is_err());
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let mut crypto = LobbyCrypto::new();
        crypto.derive_keys();
        let iv = [7u8; 16];
        let plaintext = b"service call payload";
        let encrypted = crypto.encrypt_lobby_packet(plaintext, &iv).unwrap();
        assert_eq!(encrypted.len(), 32);
        let decrypted = crypto.decrypt_lobby_packet(&encrypted, &iv).unwrap();
        assert_eq!(&decrypted[..plaintext.len()], plaintext);
        assert!(decrypted[plaintext.len()..].iter().all(|&b| b == 0));
        assert_eq!(crypto.response_id(), t9_common::SESSION_KEY[16..24]);
    }
}
