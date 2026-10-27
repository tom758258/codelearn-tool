//! The bounded subset of MI3 used by the debug session.
use std::io;

pub(super) const MAX_RECORD: usize = 64 * 1024;
#[derive(Default)]
pub(super) struct Framer {
    line: Vec<u8>,
}
impl Framer {
    pub fn push(
        &mut self,
        bytes: &[u8],
        mut record: impl FnMut(Record) -> io::Result<()>,
    ) -> io::Result<()> {
        for &byte in bytes {
            if byte == b'\n' {
                let parsed = parse(&self.line)?;
                self.line.clear();
                record(parsed)?;
            } else {
                if self.line.len() == MAX_RECORD {
                    return Err(io::Error::other("GDB MI record exceeds limit"));
                }
                self.line.push(byte);
            }
        }
        Ok(())
    }
    pub fn eof(&self) -> io::Error {
        if self.line.is_empty() {
            io::Error::new(io::ErrorKind::UnexpectedEof, "GDB MI EOF")
        } else {
            io::Error::new(io::ErrorKind::InvalidData, "GDB MI EOF within a record")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Value {
    String(Vec<u8>),
    Tuple(Vec<(String, Value)>),
    List(Vec<(Option<String>, Value)>),
}
impl Value {
    pub fn field(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Tuple(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        if let Self::String(bytes) = self {
            Some(bytes)
        } else {
            None
        }
    }
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(self.bytes()?).ok()
    }
    pub fn list(&self) -> Option<&[(Option<String>, Self)]> {
        if let Self::List(items) = self {
            Some(items)
        } else {
            None
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Record {
    Prompt,
    Result {
        token: Option<u64>,
        class: String,
        data: Value,
    },
    Async {
        kind: u8,
        class: String,
        data: Value,
    },
    Stream {
        kind: u8,
        bytes: Vec<u8>,
    },
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid GDB/MI record")
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn take(&mut self) -> io::Result<u8> {
        let byte = self.peek().ok_or_else(invalid)?;
        self.at += 1;
        Ok(byte)
    }
    fn expect(&mut self, byte: u8) -> io::Result<()> {
        if self.take()? == byte {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    fn name(&mut self) -> io::Result<String> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
        {
            self.at += 1;
        }
        if self.at == start {
            return Err(invalid());
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| invalid())?
            .into())
    }
    fn string(&mut self) -> io::Result<Vec<u8>> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            match self.take()? {
                b'"' => return Ok(bytes),
                b'\\' => {
                    let escaped = self.take()?;
                    bytes.push(match escaped {
                        b'a' => 7,
                        b'b' => 8,
                        b'e' => 27,
                        b'f' => 12,
                        b'n' => 10,
                        b'r' => 13,
                        b't' => 9,
                        b'v' => 11,
                        b'\\' | b'"' | b'\'' | b'?' => escaped,
                        b'0'..=b'7' => {
                            let mut n = (escaped - b'0') as u16;
                            for _ in 0..2 {
                                if let Some(b @ b'0'..=b'7') = self.peek() {
                                    self.at += 1;
                                    n = n * 8 + (b - b'0') as u16;
                                } else {
                                    break;
                                }
                            }
                            u8::try_from(n).map_err(|_| invalid())?
                        }
                        b'x' => {
                            let start = self.at;
                            let mut n = 0u16;
                            while let Some(b) = self.peek().filter(u8::is_ascii_hexdigit) {
                                self.at += 1;
                                n = n
                                    .checked_mul(16)
                                    .and_then(|v| v.checked_add((b as char).to_digit(16)? as u16))
                                    .ok_or_else(invalid)?;
                                if n > 255 {
                                    return Err(invalid());
                                }
                            }
                            if start == self.at {
                                return Err(invalid());
                            }
                            n as u8
                        }
                        _ => return Err(invalid()),
                    });
                }
                b'\n' | b'\r' | 0 => return Err(invalid()),
                byte => bytes.push(byte),
            }
        }
    }
    fn value(&mut self, depth: usize) -> io::Result<Value> {
        if depth > 32 {
            return Err(invalid());
        }
        match self.peek() {
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'{') => {
                self.at += 1;
                let mut fields = Vec::new();
                if self.peek() != Some(b'}') {
                    loop {
                        let name = self.name()?;
                        self.expect(b'=')?;
                        fields.push((name, self.value(depth + 1)?));
                        if self.peek() != Some(b',') {
                            break;
                        }
                        self.at += 1;
                    }
                }
                self.expect(b'}')?;
                Ok(Value::Tuple(fields))
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.peek() != Some(b']') {
                    loop {
                        let name = if matches!(self.peek(), Some(b'"' | b'{' | b'[')) {
                            None
                        } else {
                            let name = self.name()?;
                            self.expect(b'=')?;
                            Some(name)
                        };
                        items.push((name, self.value(depth + 1)?));
                        if self.peek() != Some(b',') {
                            break;
                        }
                        self.at += 1;
                    }
                }
                self.expect(b']')?;
                Ok(Value::List(items))
            }
            _ => Err(invalid()),
        }
    }
    fn fields(&mut self) -> io::Result<Value> {
        let mut fields = Vec::new();
        while self.peek() == Some(b',') {
            self.at += 1;
            let key = self.name()?;
            self.expect(b'=')?;
            fields.push((key, self.value(0)?));
        }
        Ok(Value::Tuple(fields))
    }
}

pub(super) fn parse(bytes: &[u8]) -> io::Result<Record> {
    if bytes.len() > MAX_RECORD {
        return Err(invalid());
    }
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    if bytes == b"(gdb) " || bytes == b"(gdb)" || bytes.is_empty() {
        return Ok(Record::Prompt);
    }
    let mut p = Parser { bytes, at: 0 };
    let start = p.at;
    while p.peek().is_some_and(|b| b.is_ascii_digit()) {
        p.at += 1;
    }
    let token = if p.at != start {
        Some(
            std::str::from_utf8(&bytes[start..p.at])
                .map_err(|_| invalid())?
                .parse()
                .map_err(|_| invalid())?,
        )
    } else {
        None
    };
    let kind = p.take()?;
    let record = match kind {
        b'^' => Record::Result {
            token,
            class: p.name()?,
            data: p.fields()?,
        },
        b'*' | b'+' | b'=' => Record::Async {
            kind,
            class: p.name()?,
            data: p.fields()?,
        },
        b'~' | b'@' | b'&' if token.is_none() => Record::Stream {
            kind,
            bytes: p.string()?,
        },
        _ => return Err(invalid()),
    };
    if p.at != bytes.len() {
        return Err(invalid());
    }
    Ok(record)
}

/// Encode one MI C-string, including non-ASCII bytes without lossy conversion.
pub(super) fn quote(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    for &byte in bytes {
        match byte {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0..=31 | 127 => out.extend_from_slice(format!("\\{byte:03o}").as_bytes()),
            _ => out.push(byte),
        }
    }
    out.push(b'"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_values_and_invalid_input() {
        let mut framer = Framer::default();
        let mut records = Vec::new();
        for chunk in [
            b"1^do".as_slice(),
            b"ne\r\n*stopp",
            b"ed,reason=\"signal-received\"\n",
        ] {
            framer
                .push(chunk, |record| {
                    records.push(record);
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(records.len(), 2);
        assert_eq!(framer.eof().kind(), io::ErrorKind::UnexpectedEof);
        framer.push(b"2^", |_| Ok(())).unwrap();
        assert_eq!(framer.eof().kind(), io::ErrorKind::InvalidData);
        assert!(framer.push(&vec![b'x'; MAX_RECORD], |_| Ok(())).is_err());
        let Record::Result { token, class, data } = parse(br#"42^done,stack=[frame={level="0",args=[{name="x",value="\000\377\x41\n\"\\"}]}],extra=[]"#).unwrap() else { panic!() };
        assert_eq!(token, Some(42));
        assert_eq!(class, "done");
        let frame = &data.field("stack").unwrap().list().unwrap()[0].1;
        let arg = &frame.field("args").unwrap().list().unwrap()[0].1;
        assert_eq!(
            arg.field("value").unwrap().bytes().unwrap(),
            b"\0\xffA\n\"\\"
        );
        let Record::Result { token, class, data } =
            parse(br#"11^error,msg="\e[31mNo compiled code\e[0m""#).unwrap()
        else {
            panic!()
        };
        assert_eq!(token, Some(11));
        assert_eq!(class, "error");
        assert_eq!(
            data.field("msg").unwrap().bytes().unwrap(),
            b"\x1b[31mNo compiled code\x1b[0m"
        );
        for line in [
            br#"*stopped,reason="end-stepping-range",unknown={nested=["a",[]]}"#.as_slice(),
            br#"=thread-group-started,id="i1",pid="1""#,
            br#"+download,section="x""#,
            br#"~"console\n""#,
            br#"@"target""#,
            br#"&"diagnostic""#,
        ] {
            parse(line).unwrap();
        }
        for line in [
            b"1^".as_slice(),
            b"^done,x=",
            b"^done,x=[a=]",
            b"^done,x=\"\\q\"",
            b"~\"unfinished",
            b"18446744073709551616^done",
            b"1^done garbage",
        ] {
            assert!(parse(line).is_err(), "{line:?}");
        }
        assert!(parse(&vec![b'x'; MAX_RECORD + 1]).is_err());
        let input: Vec<u8> = (0..=255).collect();
        let Record::Stream { bytes, .. } =
            parse(&[b"~".as_slice(), &quote(&input)].concat()).unwrap()
        else {
            panic!()
        };
        assert_eq!(bytes, input);
    }
}
