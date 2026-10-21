use std::sync::Mutex;
use anyhow::Result;

static HASH_PACKETS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static SESSION_KEY: Mutex<Option<[u8; 24]>> = Mutex::new(None);
static DECRYPT_KEY: Mutex<Option<[u8; 16]>> = Mutex::new(None);
static RESPONSE_ID: Mutex<[u8; 8]> = Mutex::new([0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37]);

pub fn queue_packet_to_hash(data: &[u8]) {
    if let Ok(mut packets) = HASH_PACKETS.lock() {
        packets.push(data.to_vec());
    }
}

pub fn set_session_key(key: &[u8; 24]) {
    if let Ok(mut sk) = SESSION_KEY.lock() {
        *sk = Some(*key);
    }
}

pub fn derive_keys() {
    let session_key = t9_common::SESSION_KEY;
    set_session_key(session_key);

    let mut decrypt_key = [0u8; 16];
    decrypt_key.copy_from_slice(&session_key[..16]);
    if let Ok(mut dk) = DECRYPT_KEY.lock() {
        *dk = Some(decrypt_key);
    }

    let mut response_id = [0u8; 8];
    response_id.copy_from_slice(&session_key[16..24]);
    if let Ok(mut rid) = RESPONSE_ID.lock() {
        *rid = response_id;
    }
}

pub fn get_response_id() -> [u8; 8] {
    RESPONSE_ID.lock().map(|r| *r).unwrap_or([0u8; 8])
}

pub fn decrypt_lobby_packet(encrypted: &[u8], iv_seed: &[u8]) -> Result<Vec<u8>> {
    use aes::cipher::KeyIvInit;
    use cbc::cipher::BlockDecryptMut;

    let key = DECRYPT_KEY
        .lock()
        .map_err(|_| anyhow::anyhow!("lock poisoned"))?
        .ok_or_else(|| anyhow::anyhow!("decrypt key not set — auth not completed"))?;

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

pub fn encrypt_lobby_packet(plaintext: &[u8], iv_seed: &[u8]) -> Result<Vec<u8>> {
    use aes::cipher::KeyIvInit;
    use cbc::cipher::BlockEncryptMut;

    let key = DECRYPT_KEY
        .lock()
        .map_err(|_| anyhow::anyhow!("lock poisoned"))?
        .ok_or_else(|| anyhow::anyhow!("encrypt key not set — auth not completed"))?;

    let iv: [u8; 16] = iv_seed
        .try_into()
        .map_err(|_| anyhow::anyhow!("IV must be 16 bytes"))?;

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
    let encryptor = Aes128CbcEnc::new(&key.into(), &iv.into());

    let mut buf = vec![0u8; plaintext.len() + 16]; // room for padding
    buf[..plaintext.len()].copy_from_slice(plaintext);

    let encrypted = encryptor
        .encrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf, plaintext.len())
        .map_err(|e| anyhow::anyhow!("AES encryption failed: {}", e))?;

    Ok(encrypted.to_vec())
}
