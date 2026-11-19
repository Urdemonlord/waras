//! Dekompresi LZString (`decompressFromEncodedURIComponent`).
//!
//! Respons BPJS yang terenkripsi, setelah didekripsi AES-256-CBC, BUKAN
//! langsung JSON: plaintext-nya masih dikompresi dengan algoritma
//! LZString varian `compressToEncodedURIComponent` (dipakai semua layanan
//! bridging BPJS — VClaim, PCare, Antrean, Aplicares — lewat pipeline
//! Fetcher yang sama). Tanpa langkah ini, dekripsi terhadap server BPJS
//! asli selalu gagal dengan "plaintext bukan JSON".
//!
//! Port setia dari `lz-string` 1.5.0
//! (https://github.com/pieroxy/lz-string, MIT):
//! alfabet URI-safe `A-Za-z0-9+-$`, `resetValue` 32, spasi di input
//! diganti `+` dulu. Kamus bekerja pada unit kode UTF-16 (seperti JS),
//! hasilnya dikembalikan sebagai `String` lewat `String::from_utf16`.
//! Vektor uji di bawah dihasilkan dari paket `lz-string` 1.5.0 asli
//! (Node), bukan dari implementasi ini sendiri.

const KEY_STR_URI_SAFE: &[u8; 65] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$";

/// Dekompresi string hasil `compressToEncodedURIComponent`.
///
/// Mengembalikan `None` bila input kosong atau tidak bisa didekompresi
/// (mirip `null` pada implementasi JS). Input yang terpotong di tengah
/// mengembalikan `Some("")`, sama seperti JS.
pub fn decompress_from_encoded_uri_component(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    // JS: input.replace(/ /g, "+").
    let replaced = input.replace(' ', "+");
    let units: Vec<u16> = replaced.encode_utf16().collect();
    let out = decompress(units.len(), 32, |index| {
        if index >= units.len() {
            // JS: keyStrUriSafe.indexOf("") === 0.
            return 0i32;
        }
        let ch = units[index];
        if ch > 127 {
            return -1;
        }
        match KEY_STR_URI_SAFE.iter().position(|&b| b as u16 == ch) {
            Some(pos) => pos as i32,
            None => -1,
        }
    })?;
    String::from_utf16(&out).ok()
}

/// Port `_decompress(length, resetValue, getNextValue)` dari lz-string.
/// Bekerja pada unit kode UTF-16; `get_next_value` mengembalikan nilai
/// seperti `keyStrUriSafe.indexOf(...)` di JS (0 bila indeks di luar
/// string, -1 bila karakter tidak ada di alfabet).
fn decompress(
    length: usize,
    reset_value: i32,
    mut get_next_value: impl FnMut(usize) -> i32,
) -> Option<Vec<u16>> {
    let mut dictionary: Vec<Vec<u16>> = vec![vec![0], vec![1], vec![2]];
    let mut enlarge_in: i32 = 4;
    let mut dict_size: usize = 4;
    let mut num_bits: u32 = 3;
    let mut result: Vec<u16> = Vec::new();

    let mut data_val = get_next_value(0);
    let mut data_position = reset_value;
    let mut data_index: usize = 1;

    // Baca `count` bit dari stream, LSB dulu (persis loop `while
    // (power != maxpower)` di JS).
    macro_rules! read_bits {
        ($count:expr) => {{
            let mut bits: u32 = 0;
            let maxpower: u32 = 1u32 << $count;
            let mut power: u32 = 1;
            while power != maxpower {
                let resb = data_val & data_position;
                data_position >>= 1;
                if data_position == 0 {
                    data_position = reset_value;
                    data_val = get_next_value(data_index);
                    data_index += 1;
                }
                if resb > 0 {
                    bits |= power;
                }
                power <<= 1;
            }
            bits
        }};
    }

    let next = read_bits!(2);
    let first: Vec<u16> = match next {
        0 => vec![read_bits!(8) as u16],
        1 => vec![read_bits!(16) as u16],
        2 => return Some(Vec::new()),
        _ => return None,
    };
    dictionary.push(first.clone());
    let mut w: Vec<u16> = first.clone();
    result.extend_from_slice(&first);

    loop {
        if data_index > length {
            return Some(Vec::new());
        }
        let c = read_bits!(num_bits);
        // JS: switch tanpa default — kode selain 0/1/2 lolos apa adanya
        // sebagai indeks kamus (bukan error).
        let c: usize = match c {
            0 => {
                let ch = read_bits!(8) as u16;
                dictionary.push(vec![ch]);
                dict_size += 1;
                enlarge_in -= 1;
                dict_size - 1
            }
            1 => {
                let ch = read_bits!(16) as u16;
                dictionary.push(vec![ch]);
                dict_size += 1;
                enlarge_in -= 1;
                dict_size - 1
            }
            2 => return Some(result),
            other => other as usize,
        };

        if enlarge_in == 0 {
            enlarge_in = 1i32 << num_bits;
            num_bits += 1;
        }

        let entry: Vec<u16> = if let Some(e) = dictionary.get(c) {
            e.clone()
        } else if c == dict_size {
            let mut e = w.clone();
            e.push(w[0]);
            e
        } else {
            return None;
        };
        result.extend_from_slice(&entry);

        let mut new_entry = w.clone();
        new_entry.push(entry[0]);
        dictionary.push(new_entry);
        dict_size += 1;
        enlarge_in -= 1;

        w = entry;
        if enlarge_in == 0 {
            enlarge_in = 1i32 << num_bits;
            num_bits += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vektor emas dari paket `lz-string` 1.5.0 asli (Node):
    /// `lz.compressToEncodedURIComponent(plaintext)`.
    #[test]
    fn vektor_emas_lz_string_1_5_0() {
        let cases: &[(&str, &str)] = &[
            (
                "{\"sep\":{\"noSep\":\"SEP-CONTOH-001\"}}",
                "N4IgzgpgDiBcoDsD2BlacQoKIAUC0AwgPIByAKkQBJ4AMNAjCAL5NA",
            ),
            (
                "{\"metaData\":{\"code\":\"200\",\"message\":\"Ok\"},\"response\":{\"peserta\":{\"noKartu\":\"0001234567890\",\"nama\":\"Siti Rahmawati\",\"nik\":\"3374021205850001\"}}}",
                "N4IgtgpgLghgIjWIBcoDGB7AJhFIBMADISADTgQDOlMA5rsiAPIDWIAvuQE5UAOGAO0oNQvKhC5JUIARgDSMSQFc8xQgEZ8AZgAsAVgBsAdgAcAThLkBMMDDwBlAJZRHAAgBKMABa2A7okcyGUc2Ri0tIx1CfE1CPRM9NXUOdnYgA",
            ),
            (
                "{\"response\":{\"list\":[{\"kodekelas\":\"1\",\"kapasitas\":10,\"tersedia\":3}]}}",
                "N4IgTgpgzgDg9gOyhEAuUAbAllALmgbVAGs4ATCYiDAQyjRAEYQAaEYmmOrXOtRgAxtcEMMjJYaaAMwBfALqzZQA",
            ),
            ("", "Q"),
            ("a", "IZA"),
            ("AAAAAAAAAA", "II1o"),
            ("{\"a\":1}", "N4IghiBcCMC+Q"),
        ];
        for (plain, compressed) in cases {
            assert_eq!(
                decompress_from_encoded_uri_component(compressed).as_deref(),
                Some(*plain),
                "vektor gagal: {compressed}"
            );
        }
    }

    #[test]
    fn input_kosong_dan_sampah_mengembalikan_none() {
        assert_eq!(decompress_from_encoded_uri_component(""), None);
        // Karakter di luar alfabet + panjang tidak valid: tidak boleh panic.
        assert!(decompress_from_encoded_uri_component("!!!").is_none()
            || decompress_from_encoded_uri_component("!!!") == Some(String::new()));
    }

    #[test]
    fn spasi_diperlakukan_sebagai_plus() {
        // "N4IghiBcCMC Q" (spasi ganti '+') harus sama dengan "N4IghiBcCMC+Q".
        assert_eq!(
            decompress_from_encoded_uri_component("N4IghiBcCMC Q").as_deref(),
            Some("{\"a\":1}")
        );
    }
}
