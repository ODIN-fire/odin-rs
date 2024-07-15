/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

/// module with encryption support for ODIN application build scripts

use std::{path::{Path,PathBuf}, env, fs::File};
use serde::Deserialize;
use pgp;
use magic_crypt::MagicCryptTrait;
use crate::utils::file_contents_as_bytes;
use ron;

#[derive(Deserialize,Debug)]
pub enum Encryption {
    PGP(PathBuf),
    Passphrase(String),
}


pub fn encrypt (data_path: &str, data: &[u8], encryption: Encryption) -> Vec<u8> {
    match encryption {
        Encryption::Pki(pub_key_path) => pgp_encrypt(data_path, data, pub_key_path),
        Encryption::Passphrase(pp) => pp_encrypt(data, pp.as_str())
    }
}

fn pgp_encrypt (data_path: &str, data: &[u8], pub_key_path: impl AsRef<Path>) -> Vec<u8> {
    use pgp::Deserializable;
    use rand::SeedableRng;

    let mut pub_key_file = File::open(pub_key_path).expect("cannot open public key file");

    let (pub_key, _headers) = pgp::composed::SignedPublicKey::from_armor_single(&mut pub_key_file).expect("failed to read key");
    pub_key.verify().expect("invalid public key");

    let lit_msg = pgp::composed::Message::new_literal_bytes(data_path, data);
    let compressed_msg = lit_msg.compress(pgp::types::CompressionAlgorithm::ZLIB).unwrap();

    let mut rng = rand::rngs::StdRng::seed_from_u64(100);
    let encrypted = compressed_msg
            .encrypt_to_keys(&mut rng, pgp::crypto::sym::SymmetricKeyAlgorithm::AES128, &[&pub_key.primary_key][..])
            .unwrap();
    let armored = encrypted.to_armored_bytes(None).unwrap();

    armored
}

fn pp_encrypt (data: &[u8], pp: &str) -> Vec<u8> {
    let mc = magic_crypt::new_magic_crypt!(pp,256);
    mc.encrypt_to_bytes(data)
}

pub fn get_encryption()->Option<Encryption> {
    if let Ok(conf) = std::env::var("ODIN_ENCRYPT") {
        let path = Path::new(&conf);
        if path.is_file() {
            let data = file_contents_as_bytes(path);
            ron::from_str( &data).unwrap()
        } else {
            panic!("ODIN_ENCRYPTION not found: {path}")
        }
    } else {
        None
    }
}