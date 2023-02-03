use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufWriter, Write};
use std::path::Path;
use std::str::FromStr;
use std::{env, io, process};

use getopts::Options;
use inflector::Inflector;
use regex::Regex;

const ATTRIBUTE_KIND: &str = "ATTRIBUTE";
const VALUE_KIND: &str = "VALUE";

const RADIUS_VALUE_TYPE: &str = "u32";

const USER_PASSWORD_TYPE_OPT: &str = "encrypt=1";
const TUNNEL_PASSWORD_TYPE_OPT: &str = "encrypt=2";
const HAS_TAG_TYPE_OPT: &str = "has_tag";
const CONCAT_TYPE_OPT: &str = "concat";

#[derive(Debug)]
enum EncryptionType {
    UserPassword,
    TunnelPassword,
}

#[derive(Debug)]
struct RadiusAttribute {
    name: String,
    typ: u8,
    value_type: RadiusAttributeValueType,
    fixed_octets_length: Option<usize>,
    concat_octets: bool,
    has_tag: bool,
}

#[derive(Debug)]
struct RadiusValue {
    name: String,
    value: u16,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq)]
enum RadiusAttributeValueType {
    String,
    UserPassword,
    TunnelPassword,
    Octets,
    IpAddr,
    Ipv4Prefix,
    Ipv6Addr,
    Ipv6Prefix,
    IfId,
    Date,
    Integer,
    Short,
    VSA,
}

impl FromStr for RadiusAttributeValueType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "string" => Ok(RadiusAttributeValueType::String),
            "octets" => Ok(RadiusAttributeValueType::Octets),
            "ipaddr" => Ok(RadiusAttributeValueType::IpAddr),
            "ipv4prefix" => Ok(RadiusAttributeValueType::Ipv4Prefix),
            "ipv6addr" => Ok(RadiusAttributeValueType::Ipv6Addr),
            "ipv6prefix" => Ok(RadiusAttributeValueType::Ipv6Prefix),
            "ifid" => Ok(RadiusAttributeValueType::IfId),
            "date" => Ok(RadiusAttributeValueType::Date),
            "integer" => Ok(RadiusAttributeValueType::Integer),
            "short" => Ok(RadiusAttributeValueType::Short),
            "vsa" => Ok(RadiusAttributeValueType::VSA),
            _ => Err(()),
        }
    }
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {program} [options] DICT_FILE OUT_FILE");
    print!("{}", opts.usage(&brief));
    process::exit(0);
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt(
        "o",
        "out-dir",
        "[mandatory] a directory to out the generated code",
        "/path/to/out/",
    );
    let matches = opts.parse(&args[1..]).unwrap_or_else(|f| panic!("{}", f));

    if matches.opt_present("h") {
        print_usage(&program, &opts);
    }

    let out_dir_str = match matches.opt_str("o") {
        Some(o) => o,
        None => panic!("mandatory parameter `-o` (`--out-dir`) is missing"),
    };
    let out_dir = Path::new(&out_dir_str);

    let mut dict_file_paths: Vec<&Path> = matches
        .free
        .iter()
        .map(Path::new)
        .filter(|path| {
            if !path.exists() || !path.is_file() {
                panic!("no such dictionary file => {}", path.to_str().unwrap());
            }
            true
        })
        .collect();
    dict_file_paths.sort();

    let mut rfc_names: Vec<String> = Vec::new();
    let mut attribute_name_to_rfc_name: HashMap<String, String> = HashMap::new();

    for dict_file_path in dict_file_paths {
        let ((radius_attributes, radius_attribute_to_values_map), dict_file_lines) =
            parse_dict_file(dict_file_path).unwrap();

        let value_defined_attributes_set = radius_attribute_to_values_map
            .keys()
            .collect::<HashSet<&String>>();

        let rfc_name = dict_file_path.extension().unwrap().to_str().unwrap();
        let mut w = BufWriter::new(File::create(out_dir.join(format!("{rfc_name}.rs"))).unwrap());

        generate_header(&mut w, &rfc_names, rfc_name, dict_file_lines);
        generate_attributes_code(&mut w, &radius_attributes, &value_defined_attributes_set);
        generate_values_code(
            &mut w,
            &radius_attribute_to_values_map,
            &attribute_name_to_rfc_name,
        );

        for attr in &radius_attributes {
            attribute_name_to_rfc_name.insert(attr.name.clone(), rfc_name.to_owned());
        }
        rfc_names.push(rfc_name.to_owned());
    }
}

fn generate_header(
    w: &mut BufWriter<File>,
    rfc_names: &[String],
    rfc_name: &str,
    dict_file_lines: io::Lines<io::BufReader<File>>,
) {
    let code = format!(
        "// Code generated by machine generator; DO NOT EDIT.

//! Utility for {rfc_name} packet.
//!
//! This module handles the packet according to the following definition:
//! ```text
//! {dict_file_contents}
//! ```

use std::net::{{Ipv4Addr, Ipv6Addr}};

use chrono::{{DateTime, Utc}};

use crate::core::avp::{{AVP, AVPType, AVPError}};
use crate::core::packet::Packet;
use crate::core::tag::Tag;

",
        rfc_name = rfc_name,
        dict_file_contents = dict_file_lines
            .map(|line| format!("//! {}", line.unwrap()))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    w.write_all(code.as_bytes()).unwrap();

    for rfc_name in rfc_names {
        w.write_all(format!("use crate::core::{rfc_name};\n").as_bytes())
            .unwrap();
    }
}

fn generate_values_code(
    w: &mut BufWriter<File>,
    attr_to_values_map: &BTreeMap<String, Vec<RadiusValue>>,
    attr_name_to_rfc_name: &HashMap<String, String>,
) {
    for (attr, values) in attr_to_values_map {
        generate_values_for_attribute_code(w, attr, values, attr_name_to_rfc_name.get(attr));
    }
}

fn generate_values_for_attribute_code(
    w: &mut BufWriter<File>,
    attr: &str,
    values: &[RadiusValue],
    maybe_rfc_name: Option<&String>,
) {
    let type_name = attr.to_pascal_case();

    if maybe_rfc_name.is_none() {
        w.write_all(format!("\npub type {type_name} = {RADIUS_VALUE_TYPE};\n").as_bytes())
            .unwrap();
    }

    for v in values {
        if let Some(rfc_name) = maybe_rfc_name {
            w.write_all(
                format!(
                "pub const {type_name_prefix}_{value_name}: {rfc_name}::{type_name} = {value};\n",
                type_name_prefix = type_name.to_screaming_snake_case(),
                value_name = v.name.to_screaming_snake_case(),
                rfc_name = rfc_name,
                type_name = type_name,
                value = v.value,
            )
                .as_bytes(),
            )
            .unwrap()
        } else {
            w.write_all(
                format!(
                    "pub const {type_name_prefix}_{value_name}: {type_name} = {value};\n",
                    type_name_prefix = type_name.to_screaming_snake_case(),
                    value_name = v.name.to_screaming_snake_case(),
                    type_name = type_name,
                    value = v.value,
                )
                .as_bytes(),
            )
            .unwrap();
        }
    }
    w.write_all(b"\n").unwrap();
}

fn generate_attributes_code(
    w: &mut BufWriter<File>,
    attrs: &[RadiusAttribute],
    value_defined_attributes_set: &HashSet<&String>,
) {
    for attr in attrs {
        generate_attribute_code(w, attr, value_defined_attributes_set);
    }
}

fn generate_attribute_code(
    w: &mut BufWriter<File>,
    attr: &RadiusAttribute,
    value_defined_attributes_set: &HashSet<&String>,
) {
    let attr_name = attr.name.clone();
    let type_identifier = format!("{}_TYPE", attr_name.to_screaming_snake_case());
    let type_value = attr.typ;
    let method_identifier = attr_name.to_snake_case();

    generate_common_attribute_code(w, &attr_name, &type_identifier, type_value);
    match attr.value_type {
        RadiusAttributeValueType::String => match attr.has_tag {
            true => generate_tagged_string_attribute_code(w, &method_identifier, &type_identifier),
            false => generate_string_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::UserPassword => match attr.has_tag {
            true => unimplemented!("tagged-user-password"),
            false => generate_user_password_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::TunnelPassword => match attr.has_tag {
            true => {
                generate_tunnel_password_attribute_code(w, &method_identifier, &type_identifier)
            }
            false => unimplemented!("tunnel-password"),
        },
        RadiusAttributeValueType::Octets => match attr.has_tag {
            true => unimplemented!("tagged-octets"),
            false => match attr.fixed_octets_length {
                Some(fixed_octets_length) => generate_fixed_length_octets_attribute_code(
                    w,
                    &method_identifier,
                    &type_identifier,
                    fixed_octets_length,
                ),
                None => match attr.concat_octets {
                    true => generate_concat_octets_attribute_code(
                        w,
                        &method_identifier,
                        &type_identifier,
                    ),
                    false => {
                        generate_octets_attribute_code(w, &method_identifier, &type_identifier)
                    }
                },
            },
        },
        RadiusAttributeValueType::IpAddr => match attr.has_tag {
            true => unimplemented!("tagged-ip-addr"),
            false => generate_ipaddr_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::Ipv4Prefix => match attr.has_tag {
            true => unimplemented!("tagged-ip-addr"),
            false => generate_ipv4_prefix_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::Ipv6Addr => match attr.has_tag {
            true => unimplemented!("tagged-ip-v6-addr"),
            false => generate_ipv6addr_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::Ipv6Prefix => match attr.has_tag {
            true => unimplemented!("tagged-ipv6-prefix"),
            false => generate_ipv6_prefix_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::IfId => match attr.has_tag {
            true => unimplemented!("tagged-ifid"),
            false => generate_fixed_length_octets_attribute_code(
                w,
                &method_identifier,
                &type_identifier,
                8,
            ),
        },
        RadiusAttributeValueType::Date => match attr.has_tag {
            true => unimplemented!("tagged-date"),
            false => generate_date_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::Integer => {
            match value_defined_attributes_set.contains(&attr_name) {
                true => match attr.has_tag {
                    true => generate_tagged_value_defined_integer_attribute_code(
                        w,
                        &method_identifier,
                        &type_identifier,
                        &attr_name.to_pascal_case(),
                    ),
                    false => generate_value_defined_integer_attribute_code(
                        w,
                        &method_identifier,
                        &type_identifier,
                        &attr_name.to_pascal_case(),
                    ),
                },
                false => match attr.has_tag {
                    true => generate_tagged_integer_attribute_code(
                        w,
                        &method_identifier,
                        &type_identifier,
                    ),
                    false => {
                        generate_integer_attribute_code(w, &method_identifier, &type_identifier)
                    }
                },
            }
        }
        RadiusAttributeValueType::Short => match attr.has_tag {
            true => unimplemented!("tagged-short"),
            false => generate_short_attribute_code(w, &method_identifier, &type_identifier),
        },
        RadiusAttributeValueType::VSA => generate_vsa_attribute_code(),
    }
}

fn generate_common_attribute_code(
    w: &mut BufWriter<File>,
    attr_name: &str,
    type_identifier: &str,
    type_value: u8,
) {
    let code = format!(
        "
pub const {type_identifier}: AVPType = {type_value};
/// Delete all of `{method_identifier}` values from a packet.
pub fn delete_{method_identifier}(packet: &mut Packet) {{
    packet.delete({type_identifier});
}}
",
        method_identifier = attr_name.to_snake_case(),
        type_identifier = type_identifier,
        type_value = type_value,
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_string_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` string value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &str) {{
    packet.add(AVP::from_string({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` string value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<String, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_string())
}}
/// Lookup all of the `{method_identifier}` string value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<String>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_string()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_tagged_string_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` tagged string value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, tag: Option<&Tag>, value: &str) {{
    packet.add(AVP::from_tagged_string({type_identifier}, tag, value));
}}
/// Lookup a `{method_identifier}` tagged string value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<(String, Option<Tag>), AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_tagged_string())
}}
/// Lookup all of the `{method_identifier}` tagged string value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<(String, Option<Tag>)>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_tagged_string()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_user_password_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` user-password value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) -> Result<(), AVPError> {{
    packet.add(AVP::from_user_password({type_identifier}, value, packet.get_secret(), packet.get_authenticator())?);
    Ok(())
}}
/// Lookup a `{method_identifier}` user-password value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<Vec<u8>, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_user_password(packet.get_secret(), packet.get_authenticator()))
}}
/// Lookup all of the `{method_identifier}` user-password value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<Vec<u8>>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_user_password(packet.get_secret(), packet.get_authenticator())?)
    }}
    Ok(vec)
}}
");
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_tunnel_password_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` tunnel-password value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, tag: Option<&Tag>, value: &[u8]) -> Result<(), AVPError> {{
    packet.add(AVP::from_tunnel_password({type_identifier}, tag, value, packet.get_secret(), packet.get_authenticator())?);
    Ok(())
}}
/// Lookup a `{method_identifier}` tunnel-password value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<(Vec<u8>, Tag), AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_tunnel_password(packet.get_secret(), packet.get_authenticator()))
}}
/// Lookup all of the `{method_identifier}` tunnel-password value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<(Vec<u8>, Tag)>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_tunnel_password(packet.get_secret(), packet.get_authenticator())?)
    }}
    Ok(vec)
}}
");
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_octets_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` octets value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) {{
    packet.add(AVP::from_bytes({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` octets value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Vec<u8>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_bytes())
}}
/// Lookup all of the `{method_identifier}` octets value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Vec<Vec<u8>> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_bytes())
    }}
    vec
}}
");
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_concat_octets_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) {{
    packet.extend(
        value
            .chunks(253)
            .map(|chunk| AVP::from_bytes({type_identifier}, chunk))
            .collect(),
    );
}}
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Vec<u8>> {{
    let avps = packet.lookup_all({type_identifier});
    match avps.is_empty() {{
        true => None,
        false => Some(avps.into_iter().fold(Vec::new(), |mut acc, v| {{
            acc.extend(v.encode_bytes());
            acc
        }})),
    }}
}}
"
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_fixed_length_octets_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
    fixed_octets_length: usize,
) {
    let code = format!(
        "/// Add `{method_identifier}` fixed-length octets value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) -> Result<(), AVPError> {{
    if value.len() != {fixed_octets_length} {{
        return Err(AVPError::InvalidAttributeLengthError(\"{fixed_octets_length} bytes\".to_owned(), value.len()));
    }}
    packet.add(AVP::from_bytes({type_identifier}, value));
    Ok(())
}}
/// Lookup a `{method_identifier}` fixed-length octets value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Vec<u8>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_bytes())
}}
/// Lookup all of the `{method_identifier}` fixed-length octets value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Vec<Vec<u8>> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_bytes())
    }}
    vec
}}
"
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_ipaddr_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` ipaddr value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &Ipv4Addr) {{
    packet.add(AVP::from_ipv4({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` ipaddr value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<Ipv4Addr, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_ipv4())
}}
/// Lookup all of the `{method_identifier}` ipaddr value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<Ipv4Addr>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_ipv4()?)
    }}
    Ok(vec)
}}
"
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_ipv4_prefix_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` ipv4 prefix value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) -> Result<(), AVPError> {{
    packet.add(AVP::from_ipv4_prefix({type_identifier}, value)?);
    Ok(())
}}
/// Lookup a `{method_identifier}` ipv4 prefix value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<Vec<u8>, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_ipv4_prefix())
}}
/// Lookup all of the `{method_identifier}` ipv4 prefix value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<Vec<u8>>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_ipv4_prefix()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_ipv6addr_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` ipv6addr value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &Ipv6Addr) {{
    packet.add(AVP::from_ipv6({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` ipv6addr value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<Ipv6Addr, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_ipv6())
}}
/// Lookup all of the `{method_identifier}` ipv6addr value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<Ipv6Addr>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_ipv6()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_ipv6_prefix_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` ipv6 prefix value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &[u8]) -> Result<(), AVPError> {{
    packet.add(AVP::from_ipv6_prefix({type_identifier}, value)?);
    Ok(())
}}
/// Lookup a `{method_identifier}` ipv6 prefix value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<Vec<u8>, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_ipv6_prefix())
}}
/// Lookup all of the `{method_identifier}` ipv6 prefix value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<Vec<u8>>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_ipv6_prefix()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_date_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` date value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: &DateTime<Utc>) {{
    packet.add(AVP::from_date({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` date value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<DateTime<Utc>, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_date())
}}
/// Lookup all of the `{method_identifier}` date value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<DateTime<Utc>>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_date()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_integer_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` integer value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: u32) {{
    packet.add(AVP::from_u32({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` integer value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<u32, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_u32())
}}
/// Lookup all of the `{method_identifier}` integer value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<u32>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_u32()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_tagged_integer_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` tagged integer value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, tag: Option<&Tag>, value: u32) {{
    packet.add(AVP::from_tagged_u32({type_identifier}, tag, value));
}}
/// Lookup a `{method_identifier}` tagged integer value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<(u32, Tag), AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_tagged_u32())
}}
/// Lookup all of the `{method_identifier}` tagged integer value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<(u32, Tag)>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_tagged_u32()?)
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_value_defined_integer_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
    value_type: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` value-defined integer value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: {value_type}) {{
    packet.add(AVP::from_u32({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` value-defined integer value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<{value_type}, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| Ok(v.encode_u32()? as {value_type}))
}}
/// Lookup all of the `{method_identifier}` value-defined integer value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<{value_type}>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_u32()? as {value_type})
    }}
    Ok(vec)
}}
",
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_tagged_value_defined_integer_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
    value_type: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` tagged value-defined integer value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, tag: Option<&Tag>, value: {value_type}) {{
    packet.add(AVP::from_tagged_u32({type_identifier}, tag, value));
}}
/// Lookup a `{method_identifier}` tagged value-defined integer value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<({value_type}, Tag), AVPError>> {{
    packet.lookup({type_identifier}).map(|v| {{
        let (v, t) = v.encode_tagged_u32()?;
        Ok((v as {value_type}, t))
    }})
}}
/// Lookup all of the `{method_identifier}` tagged value-defined integer value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<({value_type}, Tag)>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        let (v, t) = avp.encode_tagged_u32()?;
        vec.push((v as {value_type}, t))
    }}
    Ok(vec)
}}
"
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_short_attribute_code(
    w: &mut BufWriter<File>,
    method_identifier: &str,
    type_identifier: &str,
) {
    let code = format!(
        "/// Add `{method_identifier}` short integer value to a packet.
pub fn add_{method_identifier}(packet: &mut Packet, value: u16) {{
    packet.add(AVP::from_u16({type_identifier}, value));
}}
/// Lookup a `{method_identifier}` short integer value from a packet.
///
/// It returns the first looked up value. If there is no associated value with `{method_identifier}`, it returns `None`.
pub fn lookup_{method_identifier}(packet: &Packet) -> Option<Result<u16, AVPError>> {{
    packet.lookup({type_identifier}).map(|v| v.encode_u16())
}}
/// Lookup all of the `{method_identifier}` short integer value from a packet.
pub fn lookup_all_{method_identifier}(packet: &Packet) -> Result<Vec<u16>, AVPError> {{
    let mut vec = Vec::new();
    for avp in packet.lookup_all({type_identifier}) {{
        vec.push(avp.encode_u16()?)
    }}
    Ok(vec)
}}
"
    );
    w.write_all(code.as_bytes()).unwrap();
}

fn generate_vsa_attribute_code() {
    // NOP
}

type DictParsed = (Vec<RadiusAttribute>, BTreeMap<String, Vec<RadiusValue>>);

fn parse_dict_file(
    dict_file_path: &Path,
) -> Result<(DictParsed, io::Lines<io::BufReader<File>>), String> {
    let line_filter_re = Regex::new(r"^(?:#.*|)$").unwrap();
    let ws_re = Regex::new(r"\s+").unwrap();
    let trailing_comment_re = Regex::new(r"\s*?#.+?$").unwrap();
    let fixed_length_octets_re = Regex::new(r"^octets\[(\d+)]$").unwrap();

    let mut radius_attributes: Vec<RadiusAttribute> = Vec::new();
    let mut radius_attribute_to_values: BTreeMap<String, Vec<RadiusValue>> = BTreeMap::new();

    let lines = read_lines(dict_file_path).unwrap();
    for line_result in lines {
        let line = line_result.unwrap();

        if line_filter_re.is_match(line.as_str()) {
            continue;
        }

        let items = ws_re.split(line.as_str()).collect::<Vec<&str>>();

        if items.len() < 4 {
            return Err("the number of items is lacked in a line".to_owned());
        }

        let kind = items[0];
        match kind {
            ATTRIBUTE_KIND => {
                let mut encryption_type: Option<EncryptionType> = None;
                let mut has_tag = false;
                let mut concat_octets = false;
                if items.len() >= 5 {
                    // TODO consider to extract to a method
                    for type_opt in items[4].split(',') {
                        if type_opt == USER_PASSWORD_TYPE_OPT {
                            encryption_type = Some(EncryptionType::UserPassword);
                            continue;
                        }
                        if type_opt == TUNNEL_PASSWORD_TYPE_OPT {
                            encryption_type = Some(EncryptionType::TunnelPassword);
                            continue;
                        }
                        if type_opt == HAS_TAG_TYPE_OPT {
                            has_tag = true;
                            continue;
                        }
                        if type_opt == CONCAT_TYPE_OPT {
                            concat_octets = true;
                            continue;
                        }
                    }
                }

                let (typ, fixed_octets_length) = match RadiusAttributeValueType::from_str(items[3])
                {
                    Ok(t) => {
                        if t == RadiusAttributeValueType::String {
                            match encryption_type {
                                Some(EncryptionType::UserPassword) => {
                                    (RadiusAttributeValueType::UserPassword, None)
                                }
                                Some(EncryptionType::TunnelPassword) => {
                                    (RadiusAttributeValueType::TunnelPassword, None)
                                }
                                None => (t, None),
                            }
                        } else {
                            (t, None)
                        }
                    }
                    Err(_) => {
                        // XXX ad-hoc
                        let maybe_cap = fixed_length_octets_re.captures(items[3]);
                        if let Some(cap) = maybe_cap {
                            (
                                RadiusAttributeValueType::Octets,
                                Some(cap.get(1).unwrap().as_str().parse::<usize>().unwrap()),
                            )
                        } else {
                            return Err(format!("invalid type has come => {}", items[3]));
                        }
                    }
                };

                radius_attributes.push(RadiusAttribute {
                    name: items[1].to_string(),
                    typ: items[2].parse().unwrap(),
                    value_type: typ,
                    fixed_octets_length,
                    concat_octets,
                    has_tag,
                });
            }
            VALUE_KIND => {
                let attribute_name = items[1].to_string();
                let name = items[2].to_string();

                let value = trailing_comment_re.replace(items[3], "").to_string();
                let radius_value = RadiusValue {
                    name,
                    value: value.parse().unwrap(),
                };

                match radius_attribute_to_values.get_mut(&attribute_name) {
                    None => {
                        radius_attribute_to_values
                            .insert(attribute_name.clone(), vec![radius_value]);
                    }
                    Some(vec) => {
                        vec.push(radius_value);
                    }
                };
            }
            _ => return Err(format!("unexpected kind has come => {kind}")),
        }
    }

    Ok((
        (radius_attributes, radius_attribute_to_values),
        read_lines(dict_file_path).unwrap(),
    ))
}
