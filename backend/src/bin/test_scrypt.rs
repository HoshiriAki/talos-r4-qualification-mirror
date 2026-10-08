fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stored = "scrypt$f3ea365d2aab2fa46ad7b54d859125c2$1a9097c2edfccedec03d84bdea6cb1bd25ddba9436cd8f2812085236fb2b73abed1522b1074d0bc4e98dc9d221f789d4b1407e92c14bbfa8eb5a5f512baf6505";
    let plain = "admin123";

    let parts: Vec<&str> = stored.split('$').collect();
    println!("parts.len() = {}", parts.len());
    println!("parts[0] = {}", parts[0]);

    let salt = hex::decode(parts[1])?;
    println!("salt.len() = {}", salt.len());

    let stored_hash = hex::decode(parts[2])?;
    println!("stored_hash.len() = {}", stored_hash.len());

    let mut calculated = [0u8; 64];
    let params = scrypt::Params::new(14, 8, 1)?;
    scrypt::scrypt(plain.as_bytes(), &salt, &params, &mut calculated)?;

    println!("calc hex: {}", hex::encode(calculated));
    println!("stored: {}", hex::encode(&stored_hash));
    println!("match: {}", stored_hash == calculated);

    Ok(())
}
