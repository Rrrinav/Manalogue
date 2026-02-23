use std::collections::HashMap;
use std::io::{self, Read, Write};

pub fn write_u32<W: Write>(w: &mut W, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

pub fn write_f32<W: Write>(w: &mut W, v: f32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

pub fn write_str<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    write_u32(w, s.len() as u32)?;
    w.write_all(s.as_bytes())
}

pub fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

pub fn read_f32<R: Read>(r: &mut R) -> io::Result<f32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

pub fn read_str<R: Read>(r: &mut R) -> io::Result<String> {
    let len = read_u32(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub fn write_tf_map<W: Write>(w: &mut W, map: &HashMap<String, u32>) -> io::Result<()> {
    write_u32(w, map.len() as u32)?;
    for (word, freq) in map {
        write_str(w, word)?;
        write_u32(w, *freq)?;
    }
    Ok(())
}

pub fn read_tf_map<R: Read>(r: &mut R) -> io::Result<HashMap<String, u32>> {
    let n = read_u32(r)? as usize;
    let mut map = HashMap::with_capacity(n);
    for _ in 0..n {
        let word = read_str(r)?;
        let freq = read_u32(r)?;
        map.insert(word, freq);
    }
    Ok(map)
}

