use std::str::{Chars, FromStr};

const SIMPLE_STRING: char = '+';
const SIMPLE_ERROR: char = '-';
const INTEGER: char = ':';
const BULK_STRING: char = '$';
const ARRAY: char = '*';
const NULL: char = '_';
const BOOLEAN: char = '#';
const DOUBLE: char = ',';
const BIG_NUMBER: char = '(';
const BULK_ERROR: char = '!';
const VERBATIM_STRING: char = '=';
const MAP: char = '%';
const SET: char = '~';
const PUSH: char = '>';

#[derive(Debug)]
pub enum RESP {
    SimpleString(String),
    SimpleError(String),
    Integer(i64),
    BulkString(String),
    NullBulkString,
    Array(Vec<RESP>),
    NullArray,
    Null,
    Boolean(bool),
    Double(f64),
    BigNumber(String),
    BulkError(String),
    VerbatimString { encoding: String, data: String },
    Map(Vec<(RESP, RESP)>),
    Set(Vec<RESP>),
    Push(Vec<RESP>),
    Inline(Vec<String>),
}

impl RESP {
    fn parse_until(bytes: &mut Chars, stop: &str) -> Option<String> {
        let mut data = String::new();
        while let Some(x) = bytes.next() {
            if !stop.contains(x) {
                data.push(x);
            } else {
                let mut stop_chars = stop.chars();
                if x == stop_chars.next()? {
                    for stop_char in stop_chars {
                        if bytes.next()? == stop_char {
                            continue;
                        } else {
                            return None;
                        }
                    }
                    return Some(data);
                } else {
                    return None;
                }
            }
        }
        None
    }

    fn parse_inline(initial: char, bytes: &mut Chars) -> Option<Vec<String>> {
        let mut data = bytes.collect::<String>();
        data.insert(0, initial);

        let data = data
            .split_whitespace()
            .filter(|x| !x.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<String>>();

        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    }

    fn parse_simple(bytes: &mut Chars) -> Option<String> {
        Self::parse_until(bytes, "\r\n")
    }

    fn parse_number<T>(bytes: &mut Chars) -> Option<T>
    where
        T: FromStr,
    {
        Self::parse_simple(bytes)?.parse::<T>().ok()
    }

    fn parse_big_number(bytes: &mut Chars) -> Option<String> {
        let data = Self::parse_simple(bytes)?;
        let mut chars = data.chars();
        let first = chars.next()?;
        if !(first == '+' || first == '-' || first.is_ascii_digit())
            || !chars.all(|c| c.is_ascii_digit())
        {
            None
        } else {
            if let Some(data) = data.strip_prefix("+") {
                Some(data.to_owned())
            } else {
                Some(data)
            }
        }
    }

    fn parse_array(bytes: &mut Chars) -> Option<(isize, Vec<RESP>)> {
        let length = Self::parse_number::<isize>(bytes)?;
        let mut data = Vec::new();
        for _ in 0..length {
            data.push(Self::parse_internal(bytes, true)?)
        }
        Some((length, data))
    }

    fn parse_map(bytes: &mut Chars) -> Option<(isize, Vec<(RESP, RESP)>)> {
        let length = Self::parse_number::<isize>(bytes)?;
        let mut data = Vec::new();
        for _ in 0..length {
            data.push((
                Self::parse_internal(bytes, true)?,
                Self::parse_internal(bytes, true)?,
            ))
        }
        Some((length, data))
    }

    fn parse_bulk(bytes: &mut Chars) -> Option<(isize, String)> {
        let length = Self::parse_number::<isize>(bytes)?;
        if length == -1 {
            Some((length, String::new()))
        } else {
            let data = Self::parse_simple(bytes)?;
            Some((length, data))
        }
    }

    fn parse_internal(bytes: &mut Chars, internal: bool) -> Option<Self> {
        match bytes.next()? {
            SIMPLE_STRING => Some(Self::SimpleString(Self::parse_simple(bytes)?)),
            SIMPLE_ERROR => Some(Self::SimpleError(Self::parse_simple(bytes)?)),
            INTEGER => Some(Self::Integer(Self::parse_number(bytes)?)),
            BULK_STRING => {
                let (length, data) = Self::parse_bulk(bytes)?;
                if length < -1 {
                    None
                } else if length == -1 {
                    Some(RESP::NullBulkString)
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::BulkString(data))
                }
            }
            ARRAY => {
                let (length, data) = Self::parse_array(bytes)?;
                if length < -1 {
                    None
                } else if length == -1 {
                    Some(RESP::NullArray)
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::Array(data))
                }
            }
            NULL => {
                let data = Self::parse_simple(bytes)?;
                if data.is_empty() {
                    Some(RESP::Null)
                } else {
                    None
                }
            }
            BOOLEAN => {
                let data = Self::parse_simple(bytes)?;
                match data.as_ref() {
                    "t" => Some(Self::Boolean(true)),
                    "f" => Some(Self::Boolean(false)),
                    _ => None,
                }
            }
            DOUBLE => Some(Self::Double(Self::parse_number(bytes)?)),
            BIG_NUMBER => Some(Self::BigNumber(Self::parse_big_number(bytes)?)),
            BULK_ERROR => {
                let (length, data) = Self::parse_bulk(bytes)?;
                if length < 0 {
                    None
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::BulkError(data))
                }
            }
            VERBATIM_STRING => {
                let (length, data) = Self::parse_bulk(bytes)?;

                if length < 4 {
                    None
                } else if length as usize != data.len() {
                    None
                } else {
                    let (encoding, data) = data.split_once(":")?;
                    if encoding.len() != 3 {
                        None
                    } else {
                        Some(RESP::VerbatimString {
                            data: data.to_owned(),
                            encoding: encoding.to_owned(),
                        })
                    }
                }
            }
            MAP => {
                let (length, data) = Self::parse_map(bytes)?;
                if length < 0 {
                    None
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::Map(data))
                }
            }
            SET => {
                let (length, data) = Self::parse_array(bytes)?;
                if length < 0 {
                    None
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::Set(data))
                }
            }
            PUSH => {
                let (length, data) = Self::parse_array(bytes)?;
                if length < 0 || internal {
                    None
                } else if length as usize != data.len() {
                    None
                } else {
                    Some(RESP::Push(data))
                }
            }
            x => Some(RESP::Inline(Self::parse_inline(x, bytes)?)),
        }
    }

    pub fn parse(data: &str) -> Option<Self> {
        Self::parse_internal(&mut data.chars(), false)
    }
}

impl TryFrom<&str> for RESP {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_string() {
        let parsed = RESP::parse("+Hello\r\n");
        assert!(matches!(parsed, Some(RESP::SimpleString(_))));
        if let Some(RESP::SimpleString(x)) = parsed {
            assert_eq!(x, "Hello".to_owned())
        }
    }

    #[test]
    fn simple_string_none() {
        assert!(matches!(RESP::parse("+He\nllo\r\n"), None));
        assert!(matches!(RESP::parse("+He\rllo\r\n"), None));
        assert!(matches!(RESP::parse("+Hello\r"), None));
        assert!(matches!(RESP::parse("+Hello\n"), None));
        assert!(matches!(RESP::parse("+Hello"), None));
        assert!(matches!(RESP::parse("+"), None));
        assert!(!matches!(RESP::parse("+Hello\r\n"), None));
    }

    #[test]
    fn simple_error() {
        let parsed = RESP::parse("-Hello\r\n");
        assert!(matches!(parsed, Some(RESP::SimpleError(_))));
        if let Some(RESP::SimpleError(x)) = parsed {
            assert_eq!(x, "Hello".to_owned())
        }
    }

    #[test]
    fn simple_error_none() {
        assert!(matches!(RESP::parse("-He\nllo\r\n"), None));
        assert!(matches!(RESP::parse("-He\rllo\r\n"), None));
        assert!(matches!(RESP::parse("-Hello\r"), None));
        assert!(matches!(RESP::parse("-Hello\n"), None));
        assert!(matches!(RESP::parse("-Hello"), None));
        assert!(matches!(RESP::parse("-"), None));
        assert!(!matches!(RESP::parse("-Hello\r\n"), None));
    }

    #[test]
    fn integer() {
        let parsed = RESP::parse(":+123\r\n");
        assert!(matches!(parsed, Some(RESP::Integer(_))));
        if let Some(RESP::Integer(x)) = parsed {
            assert_eq!(x, 123)
        }
    }

    #[test]
    fn integer_plus() {
        let parsed = RESP::parse(":123\r\n");
        assert!(matches!(parsed, Some(RESP::Integer(_))));
        if let Some(RESP::Integer(x)) = parsed {
            assert_eq!(x, 123)
        }
    }

    #[test]
    fn integer_minus() {
        let parsed = RESP::parse(":-123\r\n");
        assert!(matches!(parsed, Some(RESP::Integer(_))));
        if let Some(RESP::Integer(x)) = parsed {
            assert_eq!(x, -123)
        }
    }

    #[test]
    fn integer_none() {
        assert!(matches!(RESP::parse(":1\n23\r\n"), None));
        assert!(matches!(RESP::parse(":1\r23\r\n"), None));
        assert!(matches!(RESP::parse(":123\r"), None));
        assert!(matches!(RESP::parse(":123\n"), None));
        assert!(matches!(RESP::parse(":123"), None));
        assert!(matches!(RESP::parse(":"), None));
        assert!(matches!(RESP::parse(":+1\n23\r\n"), None));
        assert!(matches!(RESP::parse(":+1\r23\r\n"), None));
        assert!(matches!(RESP::parse(":+123\r"), None));
        assert!(matches!(RESP::parse(":+123\n"), None));
        assert!(matches!(RESP::parse(":+123"), None));
        assert!(matches!(RESP::parse(":+"), None));
        assert!(matches!(RESP::parse(":-1\n23\r\n"), None));
        assert!(matches!(RESP::parse(":-1\r23\r\n"), None));
        assert!(matches!(RESP::parse(":-123\r"), None));
        assert!(matches!(RESP::parse(":-123\n"), None));
        assert!(matches!(RESP::parse(":-123"), None));
        assert!(matches!(RESP::parse(":-"), None));
        assert!(matches!(RESP::parse(":1-23\r\n"), None));
        assert!(matches!(RESP::parse(":1+23\r\n"), None));
        assert!(!matches!(RESP::parse(":+123\r\n"), None));
        assert!(!matches!(RESP::parse(":123\r\n"), None));
        assert!(!matches!(RESP::parse(":-123\r\n"), None));
    }

    #[test]
    fn big_number() {
        let parsed = RESP::parse("(+123\r\n");
        assert!(matches!(parsed, Some(RESP::BigNumber(_))));
        if let Some(RESP::BigNumber(x)) = parsed {
            assert_eq!(x, "123".to_owned())
        }
    }

    #[test]
    fn big_number_plus() {
        let parsed = RESP::parse("(123\r\n");
        assert!(matches!(parsed, Some(RESP::BigNumber(_))));
        if let Some(RESP::BigNumber(x)) = parsed {
            assert_eq!(x, "123".to_owned())
        }
    }

    #[test]
    fn big_number_minus() {
        let parsed = RESP::parse("(-123\r\n");
        assert!(matches!(parsed, Some(RESP::BigNumber(_))));
        if let Some(RESP::BigNumber(x)) = parsed {
            assert_eq!(x, "-123".to_owned())
        }
    }

    #[test]
    fn big_number_none() {
        assert!(matches!(RESP::parse("(1\n23\r\n"), None));
        assert!(matches!(RESP::parse("(1\r23\r\n"), None));
        assert!(matches!(RESP::parse("(123\r"), None));
        assert!(matches!(RESP::parse("(123\n"), None));
        assert!(matches!(RESP::parse("(123"), None));
        assert!(matches!(RESP::parse("("), None));
        assert!(matches!(RESP::parse("(+1\n23\r\n"), None));
        assert!(matches!(RESP::parse("(+1\r23\r\n"), None));
        assert!(matches!(RESP::parse("(+123\r"), None));
        assert!(matches!(RESP::parse("(+123\n"), None));
        assert!(matches!(RESP::parse("(+123"), None));
        assert!(matches!(RESP::parse("(+"), None));
        assert!(matches!(RESP::parse("(-1\n23\r\n"), None));
        assert!(matches!(RESP::parse("(-1\r23\r\n"), None));
        assert!(matches!(RESP::parse("(-123\r"), None));
        assert!(matches!(RESP::parse("(-123\n"), None));
        assert!(matches!(RESP::parse("(-123"), None));
        assert!(matches!(RESP::parse("(-"), None));
        assert!(matches!(RESP::parse("(1-23\r\n"), None));
        assert!(matches!(RESP::parse("(1+23\r\n"), None));
        assert!(!matches!(RESP::parse("(+123\r\n"), None));
        assert!(!matches!(RESP::parse("(123\r\n"), None));
        assert!(!matches!(RESP::parse("(-123\r\n"), None));
    }

    #[test]
    fn array() {
        let parsed = RESP::parse("*3\r\n+Hello\r\n-World\r\n:123\r\n");
        assert!(matches!(parsed, Some(RESP::Array { .. })));
        if let Some(RESP::Array(data)) = parsed {
            assert_eq!(data.len(), 3);
            for resp in data {
                match resp {
                    RESP::SimpleString(x) => assert_eq!(x, "Hello"),
                    RESP::SimpleError(x) => assert_eq!(x, "World"),
                    RESP::Integer(x) => assert_eq!(x, 123),
                    _ => assert!(false),
                }
            }
        }
    }

    #[test]
    fn array_empty() {
        let parsed = RESP::parse("*0\r\n");
        assert!(matches!(parsed, Some(RESP::Array { .. })));
        if let Some(RESP::Array(data)) = parsed {
            assert_eq!(data.len(), 0);
            assert!(data.is_empty())
        }
    }

    #[test]
    fn array_null() {
        let parsed = RESP::parse("*-1\r\n");
        assert!(matches!(parsed, Some(RESP::NullArray)));
    }

    #[test]
    fn array_none() {
        assert!(matches!(RESP::parse("*\r\n"), None));
        assert!(matches!(RESP::parse("*-2\r\n"), None));
        assert!(matches!(RESP::parse("*1\r\n+He\rllo\r\n"), None));
        assert!(matches!(RESP::parse("*2\r\n+Hello\r\n"), None));
        assert!(!matches!(
            RESP::parse("*3\r\n+Hello\r\n-World\r\n:123\r\n"),
            None
        ));
        assert!(!matches!(RESP::parse("*0\r\n"), None));
        assert!(!matches!(RESP::parse("*-1\r\n"), None));
    }

    #[test]
    fn push() {
        let parsed = RESP::parse(">3\r\n+Hello\r\n-World\r\n:123\r\n");
        assert!(matches!(parsed, Some(RESP::Push { .. })));
        if let Some(RESP::Push(data)) = parsed {
            assert_eq!(data.len(), 3);
            for resp in data {
                match resp {
                    RESP::SimpleString(x) => assert_eq!(x, "Hello"),
                    RESP::SimpleError(x) => assert_eq!(x, "World"),
                    RESP::Integer(x) => assert_eq!(x, 123),
                    _ => assert!(false),
                }
            }
        }
    }

    #[test]
    fn push_empty() {
        let parsed = RESP::parse(">0\r\n");
        assert!(matches!(parsed, Some(RESP::Push { .. })));
        if let Some(RESP::Push(data)) = parsed {
            assert_eq!(data.len(), 0);
            assert!(data.is_empty())
        }
    }

    #[test]
    fn push_inside() {
        let parsed = RESP::parse("*1\r\n>1\r\n+Hello\r\n");
        assert!(matches!(parsed, None));
    }

    #[test]
    fn push_none() {
        assert!(matches!(RESP::parse(">\r\n"), None));
        assert!(matches!(RESP::parse(">-1\r\n"), None));
        assert!(matches!(RESP::parse(">-2\r\n"), None));
        assert!(matches!(RESP::parse(">1\r\n+He\rllo\r\n"), None));
        assert!(matches!(RESP::parse(">2\r\n+Hello\r\n"), None));
        assert!(matches!(RESP::parse("*1\r\n>1\r\n+Hello\r\n"), None));
        assert!(!matches!(
            RESP::parse(">3\r\n+Hello\r\n-World\r\n:123\r\n"),
            None
        ));
        assert!(!matches!(RESP::parse(">0\r\n"), None));
    }

    #[test]
    fn bulk_string() {
        let parsed = RESP::parse("$5\r\nHello\r\n");
        assert!(matches!(parsed, Some(RESP::BulkString { .. })));
        if let Some(RESP::BulkString(data)) = parsed {
            assert_eq!(data.len(), 5);
            assert_eq!(data, "Hello".to_owned());
        }
    }

    #[test]
    fn bulk_string_empty() {
        let parsed = RESP::parse("$0\r\n\r\n");
        assert!(matches!(parsed, Some(RESP::BulkString { .. })));
        if let Some(RESP::BulkString(data)) = parsed {
            assert_eq!(data.len(), 0);
            assert_eq!(data, "".to_owned());
        }
    }

    #[test]
    fn bulk_string_null() {
        let parsed = RESP::parse("$-1\r\n");
        assert!(matches!(parsed, Some(RESP::NullBulkString)));
    }

    #[test]
    fn bulk_string_none() {
        assert!(matches!(RESP::parse("$\r\n"), None));
        assert!(matches!(RESP::parse("$-2\r\n"), None));
        assert!(matches!(RESP::parse("$5\r\nHe\rllo\r\n"), None));
        assert!(matches!(RESP::parse("$2\r\nHello\r\n"), None));
        assert!(matches!(RESP::parse("$8\r\nHello\r\n"), None));
        assert!(!matches!(RESP::parse("$5\r\nHello\r\n"), None));
        assert!(!matches!(RESP::parse("$0\r\n\r\n"), None));
        assert!(!matches!(RESP::parse("$-1\r\n"), None));
    }

    #[test]
    fn bulk_error() {
        let parsed = RESP::parse("!5\r\nHello\r\n");
        assert!(matches!(parsed, Some(RESP::BulkError { .. })));
        if let Some(RESP::BulkError(data)) = parsed {
            assert_eq!(data.len(), 5);
            assert_eq!(data, "Hello".to_owned());
        }
    }

    #[test]
    fn bulk_error_empty() {
        let parsed = RESP::parse("!0\r\n\r\n");
        assert!(matches!(parsed, Some(RESP::BulkError { .. })));
        if let Some(RESP::BulkError(data)) = parsed {
            assert_eq!(data.len(), 0);
            assert_eq!(data, "".to_owned());
        }
    }

    #[test]
    fn bulk_error_none() {
        assert!(matches!(RESP::parse("!\r\n"), None));
        assert!(matches!(RESP::parse("!-1\r\n"), None));
        assert!(matches!(RESP::parse("!-2\r\n"), None));
        assert!(matches!(RESP::parse("!5\r\nHe\rllo\r\n"), None));
        assert!(matches!(RESP::parse("!2\r\nHello\r\n"), None));
        assert!(matches!(RESP::parse("!8\r\nHello\r\n"), None));
        assert!(!matches!(RESP::parse("!5\r\nHello\r\n"), None));
        assert!(!matches!(RESP::parse("!0\r\n\r\n"), None));
    }

    #[test]
    fn verbatim_string() {
        let parsed = RESP::parse("=9\r\ntxt:Hello\r\n");
        assert!(matches!(parsed, Some(RESP::VerbatimString { .. })));
        if let Some(RESP::VerbatimString { data, encoding }) = parsed {
            assert_eq!(data.len() + encoding.len() + 1, 9);
            assert_eq!(data, "Hello".to_owned());
        }
    }

    #[test]
    fn verbatim_string_empty() {
        let parsed = RESP::parse("=4\r\ntxt:\r\n");
        assert!(matches!(parsed, Some(RESP::VerbatimString { .. })));
        if let Some(RESP::VerbatimString { data, encoding }) = parsed {
            assert_eq!(data.len() + encoding.len() + 1, 4);
            assert_eq!(data, "".to_owned());
        }
    }

    #[test]
    fn verbatim_string_none() {
        assert!(matches!(RESP::parse("=\r\n"), None));
        assert!(matches!(RESP::parse("=-1\r\n"), None));
        assert!(matches!(RESP::parse("=-2\r\n"), None));
        assert!(matches!(RESP::parse("=5\r\nHello\r\n"), None));
        assert!(matches!(RESP::parse("=2\r\ntxt:Hello\r\n"), None));
        assert!(matches!(RESP::parse("=10\r\ntxt:Hello\r\n"), None));
        assert!(matches!(RESP::parse("=11\r\nhtml:Hello\r\n"), None));
        assert!(matches!(RESP::parse("=0\r\n\r\n"), None));
        assert!(!matches!(RESP::parse("=9\r\ntxt:Hello\r\n"), None));
        assert!(!matches!(RESP::parse("=4\r\ntxt:\r\n"), None));
    }

    #[test]
    fn null() {
        let parsed = RESP::parse("_\r\n");
        assert!(matches!(parsed, Some(RESP::Null)));
    }

    #[test]
    fn null_none() {
        assert!(matches!(RESP::parse("_hello\r\n"), None));
        assert!(matches!(RESP::parse("_\r\r\n"), None));
        assert!(matches!(RESP::parse("_\n\r\n"), None));
        assert!(!matches!(RESP::parse("_\r\n"), None));
    }

    #[test]
    fn bool_true() {
        let parsed = RESP::parse("#t\r\n");
        assert!(matches!(parsed, Some(RESP::Boolean(true))));
    }

    #[test]
    fn bool_false() {
        let parsed = RESP::parse("#f\r\n");
        assert!(matches!(parsed, Some(RESP::Boolean(false))));
    }

    #[test]
    fn bool_none() {
        assert!(matches!(RESP::parse("#\r\n"), None));
        assert!(matches!(RESP::parse("#123\r\r\n"), None));
        assert!(matches!(RESP::parse("#hello\n\r\n"), None));
        assert!(matches!(RESP::parse("#m\n\r\n"), None));
        assert!(!matches!(RESP::parse("#f\r\n"), None));
        assert!(!matches!(RESP::parse("#t\r\n"), None));
    }

    #[test]
    fn double() {
        let parsed = RESP::parse(",1.23\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert_eq!(x, 1.23)
        }
    }

    #[test]
    fn double_min_exponent() {
        let parsed = RESP::parse(",1.23e2\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert_eq!(x, 1.23e2)
        }
    }

    #[test]
    fn double_max_exponent() {
        let parsed = RESP::parse(",1.23E2\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert_eq!(x, 1.23e2)
        }
    }

    #[test]
    fn double_plus() {
        let parsed = RESP::parse(",+1.23\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert_eq!(x, 1.23)
        }
    }

    #[test]
    fn double_minus() {
        let parsed = RESP::parse(",-1.23\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert_eq!(x, -1.23)
        }
    }

    #[test]
    fn double_inf() {
        let parsed = RESP::parse(",inf\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert!(x.is_infinite());
            assert!(x.is_sign_positive());
        }
    }

    #[test]
    fn double_plus_inf() {
        let parsed = RESP::parse(",+inf\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert!(x.is_infinite());
            assert!(x.is_sign_positive());
        }
    }

    #[test]
    fn double_minus_inf() {
        let parsed = RESP::parse(",-inf\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert!(x.is_infinite());
            assert!(x.is_sign_negative());
        }
    }

    #[test]
    fn double_nan() {
        let parsed = RESP::parse(",nan\r\n");
        assert!(matches!(parsed, Some(RESP::Double(_))));
        if let Some(RESP::Double(x)) = parsed {
            assert!(x.is_nan())
        }
    }

    #[test]
    fn double_none() {
        assert!(matches!(RESP::parse(",1.\n23\r\n"), None));
        assert!(matches!(RESP::parse(",1.\r23\r\n"), None));
        assert!(matches!(RESP::parse(",1.23\r"), None));
        assert!(matches!(RESP::parse(",1.23\n"), None));
        assert!(matches!(RESP::parse(",1.23"), None));
        assert!(matches!(RESP::parse(","), None));
        assert!(matches!(RESP::parse(",+1.\n23\r\n"), None));
        assert!(matches!(RESP::parse(",+1.\r23\r\n"), None));
        assert!(matches!(RESP::parse(",+1.23\r"), None));
        assert!(matches!(RESP::parse(",+1.23\n"), None));
        assert!(matches!(RESP::parse(",+1.23"), None));
        assert!(matches!(RESP::parse(",+"), None));
        assert!(matches!(RESP::parse(",-1.\n23\r\n"), None));
        assert!(matches!(RESP::parse(",-1.\r23\r\n"), None));
        assert!(matches!(RESP::parse(",-1.23\r"), None));
        assert!(matches!(RESP::parse(",-1.23\n"), None));
        assert!(matches!(RESP::parse(",-1.23"), None));
        assert!(matches!(RESP::parse(",-"), None));
        assert!(matches!(RESP::parse(",1.-23\r\n"), None));
        assert!(matches!(RESP::parse(",1.+23\r\n"), None));
        assert!(!matches!(RESP::parse(",+123\r\n"), None));
        assert!(!matches!(RESP::parse(",123\r\n"), None));
        assert!(!matches!(RESP::parse(",-123\r\n"), None));
        assert!(!matches!(RESP::parse(",+1.23\r\n"), None));
        assert!(!matches!(RESP::parse(",1.23\r\n"), None));
        assert!(!matches!(RESP::parse(",-1.23\r\n"), None));
        assert!(!matches!(RESP::parse(",1.23e2\r\n"), None));
        assert!(!matches!(RESP::parse(",1.23E2\r\n"), None));
        assert!(!matches!(RESP::parse(",nan\r\n"), None));
        assert!(!matches!(RESP::parse(",inf\r\n"), None));
        assert!(!matches!(RESP::parse(",+inf\r\n"), None));
        assert!(!matches!(RESP::parse(",-inf\r\n"), None));
    }

    // todo: map and set tests

    #[test]
    fn inline_singular() {
        let parsed = RESP::parse("PING");
        assert!(matches!(parsed, Some(RESP::Inline(_))));
        if let Some(RESP::Inline(x)) = parsed {
            assert_eq!(x.get(0), Some(&"PING".to_owned()))
        }


    }

    #[test]
    fn inline_multiple() {
        let parsed = RESP::parse("ECHO hello world");
        assert!(matches!(parsed, Some(RESP::Inline(_))));
        if let Some(RESP::Inline(x)) = parsed {
            assert_eq!(x.get(0), Some(&"ECHO".to_owned()));
            assert_eq!(x.get(1), Some(&"hello".to_owned()));
            assert_eq!(x.get(2), Some(&"world".to_owned()));
        }
    }
}
