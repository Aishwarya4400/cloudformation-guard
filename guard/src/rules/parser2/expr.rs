///
/// Parser Grammar for the CFN Guard rule syntax. Any enhancements to the grammar
/// **MUST** be reflected in this doc section.
///
/// Sample rule language example is as show below
///
/// ```pre
/// let global := [10, 20]                              # common vars for all rules
///
///  rule example_rule {
///    let ec2_instance_types := [/^t*/, /^m*/]   # var regex either t or m family
///
///     dependent_rule                              # named rule reference
///
///    # IN (disjunction, one of them)
///    AWS::EC2::Instance InstanceType IN %ec2_instance_types
///
///    AWS::EC2::Instance {                          # Either an EBS volume
///        let volumes := block_device_mappings      # var local, snake case allowed.
///        when %volumes.*.Ebs != null {                  # Ebs is setup
///          %volumes.*.device_name == /^\/dev\/ebs-/  # must have ebs in the name
///          %volumes.*.Ebs.encryped == true               # Ebs volume must be encryped
///          %volumes.*.Ebs.delete_on_termination == true  # Ebs volume must have delete protection
///        }
///    } or
///    AWS::EC2::Instance {                   # OR a regular volume (disjunction)
///        block_device_mappings.*.device_name == /^\/dev\/sdc-\d/ # all other local must have sdc
///    }
///  }
///
///  rule dependent_rule { ... }
/// ```
///
///  The grammar for the language in ABNF form
///
///
///
///  ```ABNF
///
///  or_term                    = "or" / "OR" / "|OR|"
///
///  var_name                   = 1*CHAR [ 1*(CHAR/ALPHA/_) ]
///  var_name_access            = "%" var_name
///
///  dotted_access              = "." (var_name / var_name_access / "*")
///
///  property_access            = var_name [ dotted_access ]
///  variable_access            = var_name_access [ dotted_access ]
///
///  access                     = variable_access /
///                               property_access
///
///  not_keyword                = "NOT" / "not" / "!"
///  basic_cmp                  = "==" / ">=" / "<=" / ">" / "<"
///  other_operators            = "IN" / "EXISTS" / "EMPTY"
///  not_other_operators        = not_keyword 1*SP other_operators
///  not_cmp                    = "!=" / not_other_operators / "NOT_IN"
///  special_operators          = "KEYS" 1*SP ("==" / other_operators / not_other_operators)
///
///  cmp                        = basic_cmp / other_operators / not_cmp / special_operators
///
///  clause                     = access 1*(LWSP/comment) cmp 1*(LWSP/comment) [(access/value)]
///  rule_clause                = rule_name / not_keyword rule_name / clause
///  rule_disjunction_clauses   = rule_clause 1*(or_term 1*(LWSP/comment) rule_clause)
///  rule_conjunction_clauses   = rule_clause 1*( (LSWP/comment) rule_clause )
///
///  type_clause                = type_name 1*SP clause
///  type_block                 = type_name *SP [when] "{" *(LWSP/comment) 1*clause "}"
///
///  type_expr                  = type_clause / type_block
///
///  disjunctions_type_expr     = type_expr 1*(or_term 1*(LWSP/comment) type_expr)
///
///  primitives                 = string / integer / float / regex
///  list_type                  = "[" *(LWSP/comment) *value *(LWSP/comment) "]"
///  map_type                   = "{" key_part *(LWSP/comment) ":" *(LWSP/comment) value
///                                   *(LWSP/comment) "}"
///  key_part                   = string / var_name
///  value                      = primitives / map_type / list_type
///
///  string                     = DQUOTE <any char not DQUOTE> DQUOTE /
///                               "'" <any char not '> "'"
///  regex                      = "/" <any char not / or escaped by \/> "/"
///
///  comment                    =  "#" *CHAR (LF/CR)
///  assignment                 = "let" one_or_more_ws  var_name zero_or_more_ws
///                                     ("=" / ":=") zero_or_more_ws (access/value)
///
///  when_type                  = when 1*( (LWSP/comment) clause (LWSP/comment) )
///  when_rule                  = when 1*( (LWSP/comment) rule_clause (LWSP/comment) )
///  named_rule                 = "rule" 1*SP var_name "{"
///                                   assignment 1*(LWPS/comment)   /
///                                   (type_expr 1*(LWPS/comment))  /
///                                   (disjunctions_type_expr) *(LWSP/comment) "}"
///
///  expressions                = 1*( (assignment / named_rule / type_expr / disjunctions_type_expr / comment) (LWPS/comment) )
///  ```
///
///

//
// Extern crate dependencies
//
use nom::{FindSubstring, InputTake};
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::is_digit;
use nom::character::complete::{alpha1, char, space1, one_of, newline, space0, multispace0};
use nom::combinator::{cut, map, opt, value, peek};
use nom::error::{ParseError, context};
use nom::multi::{fold_many1, separated_nonempty_list, separated_list};
use nom::sequence::{delimited, pair, preceded, tuple, terminated};

use super::*;
use super::common::*;
use super::super::expr::*;
use super::super::values::*;
use super::values::parse_value;

//
// ABNF     =  1*CHAR [ 1*(CHAR / _) ]
//
// All names start with an alphabet and then can have _ intermixed with it. This
// combinator does not fail, it the responsibility of the consumer to fail based on
// the error
//
// Expected error codes:
//    nom::error::ErrorKind::Alpha => if the input does not start with a char
//
fn var_name(input: Span2) -> IResult<Span2, String> {
    let (remainder, first_part) = alpha1(input)?;
    let (remainder, next_part) = take_while(|c: char| c.is_alphanumeric() || c == '_')(remainder)?;
    let mut var_name = (*first_part.fragment()).to_string();
    var_name.push_str(*next_part.fragment());
    Ok((remainder, var_name))
}

//
//  var_name_access            = "%" var_name
//
//  This combinator does not fail, it is the responsibility of the consumer to fail based
//  on the error.
//
//  Expected error types:
//     nom::error::ErrorKind::Char => if if does not start with '%'
//
//  see var_name for other error codes
//
fn var_name_access(input: Span2) -> IResult<Span2, String> {
    preceded(char('%'), var_name)(input)
}

//
// Comparison operators
//
fn in_keyword(input: Span2) -> IResult<Span2, CmpOperator> {
    value(CmpOperator::In, alt((
        tag("in"),
        tag("IN")
    )))(input)
}

fn not(input: Span2) -> IResult<Span2, ()> {
    match alt((
        preceded(tag("not"), space1),
        preceded(tag("NOT"), space1)))(input) {
        Ok((remainder, _not)) => Ok((remainder, ())),

        Err(nom::Err::Error(_)) => {
            let (input, _bang_char) = char('!')(input)?;
            Ok((input, ()))
        }

        Err(e) => Err(e)
    }
}

fn eq(input: Span2) -> IResult<Span2, ValueOperator> {
    alt((
        value(ValueOperator::Cmp(CmpOperator::Eq), tag("==")),
        value(ValueOperator::Not(CmpOperator::Eq), tag("!=")),
    ))(input)
}

fn keys(input: Span2) -> IResult<Span2, ()> {
    value((), preceded(
        alt((
            tag("KEYS"),
            tag("keys"))), space1))(input)
}

fn keys_keyword(input: Span2) -> IResult<Span2, ValueOperator> {
    let (input, _keys_word) = keys(input)?;
    let (input, comparator) = alt((
        eq,
        other_operations,
    ))(input)?;

    let is_not = if let ValueOperator::Not(_) = &comparator { true } else { false };
    let comparator = match comparator {
        ValueOperator::Cmp(op) | ValueOperator::Not(op) => {
            match op {
                CmpOperator::Eq => CmpOperator::KeysEq,
                CmpOperator::In => CmpOperator::KeysIn,
                CmpOperator::Exists => CmpOperator::KeysExists,
                CmpOperator::Empty => CmpOperator::KeysEmpty,
                _ => unreachable!(),
            }
        }
    };

    let comparator = if is_not { ValueOperator::Not(comparator) } else { ValueOperator::Cmp(comparator) };
    Ok((input, comparator))
}

fn exists(input: Span2) -> IResult<Span2, CmpOperator> {
    value(CmpOperator::Exists, alt((tag("EXISTS"), tag("exists"))))(input)
}

fn empty(input: Span2) -> IResult<Span2, CmpOperator> {
    value(CmpOperator::Empty, alt((tag("EMPTY"), tag("empty"))))(input)
}

fn other_operations(input: Span2) -> IResult<Span2, ValueOperator> {
    let (input, not) = opt(not)(input)?;
    let (input, operation) = alt((
        in_keyword,
        exists,
        empty
    ))(input)?;
    let cmp = if not.is_some() { ValueOperator::Not(operation) } else { ValueOperator::Cmp(operation) };
    Ok((input, cmp))
}


fn value_cmp(input: Span2) -> IResult<Span2, ValueOperator> {
    alt((
        //
        // Basic cmp checks. Order does matter, you always go from more specific to less
        // specific. '>=' before '>' to ensure that we do not compare '>' first and conclude
        //
        eq,
        value(ValueOperator::Cmp(CmpOperator::Ge), tag(">=")),
        value(ValueOperator::Cmp(CmpOperator::Le), tag("<=")),
        value(ValueOperator::Cmp(CmpOperator::Gt), char('>')),
        value(ValueOperator::Cmp(CmpOperator::Lt), char('<')),

        //
        // Other operations
        //
        keys_keyword,
        other_operations,
    ))(input)
}

fn extract_message(input: Span2) -> IResult<Span2, &str> {
    match input.find_substring(">>") {
        None => Err(nom::Err::Failure(ParserError {
            span: input,
            kind: nom::error::ErrorKind::Tag,
            context: format!("Unable to find a closing >> tag for message"),
        })),
        Some(v) => {
            let split = input.take_split(v);
            Ok((split.0, *split.1.fragment()))
        }
    }
}

fn custom_message(input: Span2) -> IResult<Span2, &str> {
    delimited(tag("<<"), extract_message, tag(">>"))(input)
}

//
//  dotted_access              = "." (var_name / var_name_access / "*")
//
// This combinator does not fail. It is the responsibility of the consumer to fail based
// on error.
//
// Expected error types:
//    nom::error::ErrorKind::Char => if the start is not '.'
//
// see var_name, var_name_access for other error codes
//
fn dotted_access(input: Span2) -> IResult<Span2, Vec<String>> {
    fold_many1(
        preceded(
            char('.'),
            alt((
                var_name,
                map(var_name_access, |s| format!("%{}", s)),
                value("*".to_string(), char('*')),
                map(take_while1(|c: char| is_digit(c as u8)), |s: Span2| (*s.fragment()).to_string())
            ))),
        Vec::new(),
        |mut acc: Vec<String>, part| {
            acc.push(part);
            acc
        },
    )(input)
}

//
//   access     =   (var_name / var_name_access) [dotted_access]
//
fn access(input: Span2) -> IResult<Span2, PropertyAccess> {
    alt((
        map(pair(var_name_access, opt(dotted_access)),
            |(var_name, dotted)| PropertyAccess {
                var_access: Some(var_name),
                property_dotted_notation:
                if let Some(properties) = dotted { properties } else { vec![] },
            }),
        map(pair(var_name, opt(dotted_access)),
            |(first, dotted)| PropertyAccess {
                var_access: None,
                property_dotted_notation:
                if let Some(mut properties) = dotted {
                    properties.insert(0, first);
                    properties
                } else {
                    vec![first]
                },
            },
        )
    ))(input)
}

//
//  simple_unary               = "EXISTS" / "EMPTY"
//  keys_unary                 = "KEYS" 1*SP simple_unary
//  keys_not_unary             = "KEYS" 1*SP not_keyword 1*SP unary_operators
//  unary_operators            = simple_unary / keys_unary / not_keyword simple_unary / keys_not_unary
//
//
//  clause                     = access 1*SP unary_operators *(LWSP/comment) custom_message /
//                               access 1*SP binary_operators 1*(LWSP/comment) (access/value) *(LWSP/comment) custom_message
//
// Errors:
//     nom::error::ErrorKind::Alpha, if var_name_access / var_name does not work out
//     nom::error::ErrorKind::Char, if whitespace / comment does not work out for needed spaces
//
// Failures:
//     nom::error::ErrorKind::Char  if access / parse_value does not work out
//
//
fn clause(input: Span2) -> IResult<Span2, GuardClause> {
    let location = Location {
        file_name: input.extra,
        line: input.location_line(),
        column: input.get_utf8_column() as u32,
    };

    let (rest, not) = opt(not)(input)?;
    let (rest, (lhs, _ignored_space, cmp, _ignored)) = tuple((
        access,
        // It is an error to not have a ws/comment following it
        context("expecting one or more WS or comment blocks", one_or_more_ws_or_comment),
        // error if there is no value_cmp
        context("expecting comparison binary operators like >, <= or unary operators KEYS, EXISTS, EMPTY or NOT",
                value_cmp),
        // error if this isn't followed by space or comment or newline
        context("expecting one or more WS or comment blocks", one_or_more_ws_or_comment),
    ))(input)?;

    let no_rhs_expected = match &cmp {
        ValueOperator::Cmp(op) | ValueOperator::Not(op) =>
            match op {
                CmpOperator::KeysExists |
                CmpOperator::KeysEmpty |
                CmpOperator::Empty |
                CmpOperator::Exists => true,

                _ => false
            }
    };

    if no_rhs_expected {
        let (rest, custom_message) = cut(
            map(preceded(zero_or_more_ws_or_comment, opt(custom_message)),
                |msg| {
                    msg.map(String::from)
                }))(input)?;
        Ok((rest,
            GuardClause::Clause(Clause {
                access: lhs,
                comparator: cmp,
                compare_with: None,
                custom_message,
                location,
            }, not.is_some())
        ))
    } else {
        let (rest, (compare_with, custom_message)) =
            context("expecting either a property access \"engine.core\" or value like \"string\" or [\"this\", \"that\"]",
                    cut(alt((
                        map(tuple((
                            access, preceded(zero_or_more_ws_or_comment, opt(custom_message)))),
                            |(rhs, msg)| {
                                (Some(LetValue::PropertyAccess(rhs)), msg.map(String::from).or(None))
                            }),
                        map(tuple((
                            parse_value, preceded(zero_or_more_ws_or_comment, opt(custom_message)))),
                            move |(rhs, msg)| {
                                (Some(LetValue::Value(rhs)), msg.map(String::from).or(None))
                            })
                    ))))(rest)?;
        Ok((rest,
            GuardClause::Clause(Clause {
                access: lhs,
                comparator: cmp,
                compare_with,
                custom_message,
                location,
            }, not.is_some())
        ))
    }
}

//
//  rule_clause   =   (var_name (LWSP/comment)) /
//                    (var_name [1*SP << anychar >>] (LWSP/comment)
//
//
//  rule_clause get to be the most pesky of them all. It has the least
//  form and there can interpret partials of other form as a rule_clause
//  To ensure we don't do that we need to peek ahead after a rule name
//  parsing to see which of these forms is present for the rule clause
//  to succeed
//
//      rule_name[ \t]*\n
//      rule_name[ \t\n]+or[ \t\n]+
//      rule_name(#[^\n]+)
//
//      rule_name\s+<<msg>>[ \t\n]+or[ \t\n]+
//
fn rule_clause(input: Span2) -> IResult<Span2, GuardClause> {
    let location = Location {
        file_name: input.extra,
        line: input.location_line(),
        column: input.get_utf8_column() as u32,
    };

    let (remaining, not) = opt(not)(input)?;
    let (remaining, ct_type) = var_name(remaining)?;

    //
    // we peek to preserve the input, if it is or, space+newline or comment
    // we return
    //
    if let Ok((same, _ignored)) = peek(alt((
        preceded(space0, value((), newline)),
        preceded(space0, value((), comment2)),
        value((), or_join),
    )))(remaining) {
        return Ok((same, GuardClause::NamedRule(ct_type, location, not.is_some(), None)))
    }

    //
    // Else it must have a custom message
    //
    let (remaining, message) = preceded(space0, custom_message)(remaining)?;
    Ok((remaining, GuardClause::NamedRule(ct_type, location, not.is_some(), Some(message.to_string()))))
}

//
// clauses
//
fn clauses(input: Span2) -> IResult<Span2, Conjunctions> {
    let mut clauses = Conjunctions::new();
    let mut remaining = input;
    loop {
        let (rest, set) = separated_list(
            or_join,

            //
            // Order does matter here. Both rule_clause and access clause have the same syntax
            // for the first part e.g
            //
            // s3_encrypted_bucket  or configuration.containers.*.port == 80
            //
            // the first part is a rule clause and the second part is access clause. Consider
            // this example
            //
            // s3_encrypted_bucket or bucket_encryption EXISTS
            //
            // The first part if rule clause and second part is access. if we use the rule_clause
            // to be first it would interpret bucket_encryption as the rule_clause. Now to prevent that
            // we are using the alt form to first parse to see if it is clause and then try rules_clause
            //
            preceded(zero_or_more_ws_or_comment, alt((clause, rule_clause, ))),
        )(remaining)?;

        remaining = rest;

        match set.len() {
            0 => return Ok((remaining, clauses)),
            1 => clauses.push(ConjunctionClause::And(set[0].clone())),
            _ => clauses.push(ConjunctionClause::Or(set, false)),
        }
    }
}

//
// when block
//


//
//  ABNF        = "or" / "OR" / "|OR|"
//
fn or_term(input: Span2) -> IResult<Span2, Span2> {
    alt((
        tag("or"),
        tag("OR"),
        tag("|OR|")
    ))(input)
}

fn or_join(input: Span2) -> IResult<Span2, Span2> {
    delimited(
        one_or_more_ws_or_comment,
        or_term,
        one_or_more_ws_or_comment
    )(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_white_space_with_comments() {
        let examples = [
            "",
            r###"  # this is a comment that needs to be discarded
            "###,
            r###"


                # all of this must be discarded as well
            "###,
            "let a := 10", // this must fail one_or_more, success zero_or_more
        ];

        let expectations = [
            [
                Err(nom::Err::Error(
                    ParserError {
                        span: from_str2(""),
                        kind: nom::error::ErrorKind::Char,
                        context: "".to_string(),
                    })), // white_space_or_comment
                Ok((from_str2(""), ())), // zero_or_more
                Err(nom::Err::Error(
                    ParserError {
                        span: from_str2(""),
                        kind: nom::error::ErrorKind::Char,
                        context: "".to_string(),
                    })), // white_space_or_comment
            ],
            [
                Ok((unsafe { Span2::new_from_raw_offset(2, 1, "# this is a comment that needs to be discarded\n            ", "") }, ())), // white_space_or_comment, only consumes white-space)
                Ok((unsafe { Span2::new_from_raw_offset(examples[1].len(), 2, "", "") }, ())), // consumes everything
                Ok((unsafe { Span2::new_from_raw_offset(examples[1].len(), 2, "", "") }, ())), // consumes everything
            ],
            [
                //
                // Offset = 3 * '\n' + (col = 17) - 1 = 19
                //
                Ok((unsafe {
                    Span2::new_from_raw_offset(19, 4, r###"# all of this must be discarded as well
            "###, "")
                }, ())), // white_space_or_comment, only consumes white-space
                Ok((unsafe { Span2::new_from_raw_offset(examples[2].len(), 5, "", "") }, ())), // consumes everything
                Ok((unsafe { Span2::new_from_raw_offset(examples[2].len(), 5, "", "") }, ())), // consumes everything
            ],
            [
                Err(nom::Err::Error(
                    ParserError {
                        span: from_str2(examples[3]),
                        kind: nom::error::ErrorKind::Char,
                        context: "".to_string(),
                    })), // white_space_or_comment
                Ok((from_str2(examples[3]), ())), // zero_or_more
                Err(nom::Err::Error(
                    ParserError {
                        span: from_str2(examples[3]),
                        kind: nom::error::ErrorKind::Char,
                        context: "".to_string(),
                    })), // white_space_or_comment
            ],
        ];

        for (index, expected) in expectations.iter().enumerate() {
            for (idx, each) in [white_space_or_comment, zero_or_more_ws_or_comment, one_or_more_ws_or_comment].iter().enumerate() {
                let actual = each(from_str2(examples[index]));
                assert_eq!(&actual, &expected[idx]);
            }
        }
    }

    #[test]
    fn test_var_name() {
        let examples = [
            "", // err
            "v", // ok
            "var_10", // ok
            "_v", // error
            "engine_name", // ok
            "rule_name_", // ok
            "var_name # remaining", // ok
            "var name", // Ok, var == "var", remaining = " name"
            "10", // err
        ];

        let expectations = [
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(""),
                    kind: nom::error::ErrorKind::Alpha,
                    context: "".to_string(),
                })),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[1].len(),
                        1,
                        "",
                        "",
                    )
                },
                "v".to_string()
            )),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                "var_10".to_string()
            )),
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2("_v"),
                    kind: nom::error::ErrorKind::Alpha,
                    context: "".to_string(),
                })), // white_space_or_comment
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                "engine_name".to_string()
            )),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len(),
                        1,
                        "",
                        "",
                    )
                },
                "rule_name_".to_string()
            )),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        8,
                        1,
                        " # remaining",
                        "",
                    )
                },
                "var_name".to_string()
            )),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        3,
                        1,
                        " name",
                        "",
                    )
                },
                "var".to_string()
            )),
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2("10"),
                    kind: nom::error::ErrorKind::Alpha,
                    context: "".to_string(),
                })),
        ];

        for (idx, text) in examples.iter().enumerate() {
            let span = from_str2(*text);
            let actual = var_name(span);
            assert_eq!(&actual, &expectations[idx]);
        }
    }

    #[test]
    fn test_var_name_access() {
        let examples = [
            "", // Err
            "var", // err
            "%var", // ok
            "%_var", // err
            "%var_10", // ok
            " %var", // err
            "%var # remaining", // ok
            "%var this", // ok
        ];

        let expectations = [
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(""),
                    kind: nom::error::ErrorKind::Char,
                    context: "".to_string(),
                })), // white_space_or_comment

            Err(nom::Err::Error(
                ParserError {
                    span: from_str2("var"),
                    kind: nom::error::ErrorKind::Char,
                    context: "".to_string(),
                })),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                "var".to_string()
            )),
            Err(nom::Err::Error(
                ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            1,
                            1,
                            "_var",
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Alpha,
                    context: "".to_string(),
                })),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                "var_10".to_string()
            )),
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(" %var"),
                    kind: nom::error::ErrorKind::Char,
                    context: "".to_string(),
                })),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        "%var".len(),
                        1,
                        " # remaining",
                        "",
                    )
                },
                "var".to_string()
            )),
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        "%var".len(),
                        1,
                        " this",
                        "",
                    )
                },
                "var".to_string()
            )),
        ];

        for (idx, text) in examples.iter().enumerate() {
            let span = from_str2(*text);
            let actual = var_name_access(span);
            assert_eq!(&actual, &expectations[idx]);
        }
    }

    fn to_string_vec(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect::<Vec<String>>()
    }

    #[test]
    fn test_dotted_access() {
        let examples = [
            "", // err
            ".", // err
            ".configuration.engine", // ok,
            ".config.engine.", // ok
            ".config.easy", // ok
            ".%engine_map.%engine", // ok
            ".*.*.port", // ok
            ".port.*.ok", // ok
            ".first. second", // ok, why, as the firs part is valid, the remainder will be ". second"
            " .first.second", // err
            ".first.0.path ", // ok
            ".first.*.path == ", // ok
            ".first.* == ", // ok
        ];

        let expectations = [
            // fold_many1 returns Many1 as the error, many1 appends to error hence only propagates
            // the embedded parser's error
            // "", // err
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(""),
                    kind: nom::error::ErrorKind::Many1,
                    context: "".to_string(),
                }
            )),

            // ".", // err
            Err(nom::Err::Error(
                ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            0,
                            1,
                            ".",
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Many1, // last one char('*')
                    context: "".to_string(),
                }
            )),

            // ".configuration.engine", // ok,
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                to_string_vec(&["configuration", "engine"])
            )),


            // ".config.engine.", // Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[3].len() - 1,
                        1,
                        ".",
                        "",
                    )
                },
                to_string_vec(&["config", "engine"])
            )),

            // ".config.easy", // Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                to_string_vec(&["config", "easy"])
            )),

            // ".%engine_map.%engine"
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len(),
                        1,
                        "",
                        "",
                    )
                },
                to_string_vec(&["%engine_map", "%engine"])
            )),

            // ".*.*.port", // ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[6].len(),
                        1,
                        "",
                        "",
                    )
                },
                to_string_vec(&["*", "*", "port"])
            )),

            //".port.*.ok", // ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[7].len(),
                        1,
                        "",
                        "",
                    )
                },
                to_string_vec(&["port", "*", "ok"])
            )),

            //".first. second", // Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        ".first".len(),
                        1,
                        ". second",
                        "",
                    )
                },
                to_string_vec(&["first"])
            )),

            //" .first.second", // err
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(examples[9]),
                    kind: nom::error::ErrorKind::Many1,
                    context: "".to_string(),
                }
            )),


            //".first.0.path ", // ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[9].len() - 1,
                        1,
                        " ",
                        "",
                    )
                },
                to_string_vec(&["first", "0", "path"]),
            )),

            //".first.*.path == ", // ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        ".first.*.path".len(),
                        1,
                        " == ",
                        "",
                    )
                },
                to_string_vec(&["first", "*", "path"]),
            )),

            // ".first.* == ", // ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        ".first.*".len(),
                        1,
                        " == ",
                        "",
                    )
                },
                to_string_vec(&["first", "*"]),
            )),
        ];

        for (idx, text) in examples.iter().enumerate() {
            let span = from_str2(*text);
            let actual = dotted_access(span);
            assert_eq!(&actual, &expectations[idx]);
        }
    }

    #[test]
    fn test_access() {
        let examples = [
            "", // 0, err
            ".", // 1, err
            ".engine", // 2 err
            " engine", // 4 err

            // testing property access
            "engine", // 4, ok
            "engine.type", // 5 ok
            "engine.type.*", // 6 ok
            "engine.*.type.port", // 7 ok
            "engine.*.type.%var", // 8 ok
            "engine.0", // 9 ok
            "engine .0", // 10 ok engine will be property access part
            "engine.ok.*",// 11 Ok
            "engine.%name.*", // 12 ok

            // testing variable access
            "%engine.type", // 13 ok
            "%engine.*.type.0", // 14 ok
            "%engine.%type.*", // 15 ok
            "%engine.%type.*.port", // 16 ok
            "%engine.*.", // 17 ok . is remainder

            " %engine", // 18 err
        ];

        let expectations = [
            Err(nom::Err::Error(ParserError { // 0
                span: from_str2(""),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),
            Err(nom::Err::Error(ParserError { // 1
                span: from_str2("."),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),
            Err(nom::Err::Error(ParserError { // 2
                span: from_str2(".engine"),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),
            Err(nom::Err::Error(ParserError { // 3
                span: from_str2(" engine"),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),
            Ok(( // 4
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[4].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine"]),
                     var_access: None,
                 }
            )),
            Ok(( // 5
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[5].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine", "type"]),
                     var_access: None,
                 }
            )),
            Ok(( // 6
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[6].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine", "type", "*"]),
                     var_access: None,
                 }
            )),
            Ok(( // 7
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[7].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine", "*", "type", "port"]),
                     var_access: None,
                 }
            )),
            Ok(( // 8
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[8].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine", "*", "type", "%var"]),
                     var_access: None,
                 }
            )),
            Ok(( // 9
                 unsafe {
                     Span2::new_from_raw_offset(
                         examples[9].len(),
                         1,
                         "",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine", "0"]),
                     var_access: None,
                 }
            )),
            Ok(( // 10 "engine .0", // 10 ok engine will be property access part
                 unsafe {
                     Span2::new_from_raw_offset(
                         "engine".len(),
                         1,
                         " .0",
                         "",
                     )
                 },
                 PropertyAccess {
                     property_dotted_notation: to_string_vec(&["engine"]),
                     var_access: None,
                 }
            )),

            // "engine.ok.*",// 11 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[11].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["engine", "ok", "*"]),
                    var_access: None,
                }
            )),

            // "engine.%name.*", // 12 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[12].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["engine", "%name", "*"]),
                    var_access: None,
                }
            )),

            // "%engine.type", // 13 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[13].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["type"]),
                    var_access: Some("engine".to_string()),
                }
            )),


            // "%engine.*.type.0", // 14 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[14].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["*", "type", "0"]),
                    var_access: Some("engine".to_string()),
                }
            )),


            // "%engine.%type.*", // 15 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[15].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["%type", "*"]),
                    var_access: Some("engine".to_string()),
                }
            )),


            // "%engine.%type.*.port", // 16 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[16].len(),
                        1,
                        "",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["%type", "*", "port"]),
                    var_access: Some("engine".to_string()),
                }
            )),


            // "%engine.*.", // 17 ok . is remainder
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[17].len() - 1,
                        1,
                        ".",
                        "",
                    )
                },
                PropertyAccess {
                    property_dotted_notation: to_string_vec(&["*"]),
                    var_access: Some("engine".to_string()),
                }
            )),


            // " %engine", // 18 err
            Err(nom::Err::Error(ParserError { // 18
                span: from_str2(" %engine"),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = Span2::new_extra(*each, "");
            let result = access(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }

    #[test]
    fn test_other_operations() {
        let examples = [
            "", // 0 err
            " exists", // 1 err

            "exists", // 2 ok
            "not exists", // 3 ok
            "!exists", // 4 ok
            "!EXISTS", // 5 ok

            "notexists", // 6 err

            "in", // 7, ok
            "not in", // 8 ok
            "!in", // 9 ok,

            "EMPTY", // 10 ok,
            "! EMPTY", // 11 err
            "NOT EMPTY", // 12 ok
            "IN [\"t\", \"n\"]", // 13 ok
        ];

        let expectations = [

            // "", // 0 err
            Err(nom::Err::Error(ParserError {
                span: from_str2(""),
                context: "".to_string(),
                kind: nom::error::ErrorKind::Tag,
            })),

            // " exists", // 1 err
            Err(nom::Err::Error(ParserError {
                span: from_str2(" exists"),
                context: "".to_string(),
                kind: nom::error::ErrorKind::Tag,
            })),

            // "exists", // 2 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Exists),
            )),

            // "not exists", // 3 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[3].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::Exists),
            )),

            // "!exists", // 4 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::Exists),
            )),

            // "!EXISTS", // 5 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::Exists),
            )),


            // "notexists", // 6 err
            Err(nom::Err::Error(
                ParserError {
                    span: from_str2(examples[6]),
                    //
                    // why Tag?, not is optional, this is without space
                    // so it discards opt and then tries, in, exists or empty
                    // all of them fail with tag
                    //
                    kind: nom::error::ErrorKind::Tag,
                    context: "".to_string(),
                }
            )),

            // "in", // 7, ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[7].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::In),
            )),

            // "not in", // 8 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[8].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::In),
            )),

            // "!in", // 9 ok,
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[9].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::In),
            )),

            // "EMPTY", // 10 ok,
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[10].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Empty),
            )),

            // "! EMPTY", // 11 err
            Err(nom::Err::Error(
                ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            1,
                            1,
                            " EMPTY",
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Tag,
                    context: "".to_string(),
                }
            )),

            // "NOT EMPTY", // 12 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[12].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::Empty),
            )),

            // "IN [\"t\", \"n\"]", // 13 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        2,
                        1,
                        " [\"t\", \"n\"]",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::In),
            )),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = from_str2(*each);
            let result = other_operations(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }

    #[test]
    fn test_keys_keyword() {
        let examples = [
            "", // 0 err
            "KEYS", // 1 err
            "KEYS IN", // 2 Ok
            "KEYS NOT IN", // 3 Ok
            "KEYS EXISTS", // 4 Ok
            "KEYS !EXISTS", // 5 Ok,
            "KEYS ==", // 6 Ok
            "KEYS !=", // 7 Ok
            "keys ! in", // 8 err after !
            "KEYS EMPTY", // 9 ok
            "KEYS !EMPTY", // 10 ok
            " KEYS IN", // 11 err
            "KEYS ", // 12 err
        ];

        let expectations = [
            // "", // 0 err
            Err(nom::Err::Error(ParserError {
                span: from_str2(""),
                kind: nom::error::ErrorKind::Tag,
                context: "".to_string(),
            })),

            // "KEYS", // 1 err
            Err(nom::Err::Error(ParserError {
                span: unsafe {
                    Span2::new_from_raw_offset(
                        examples[1].len(),
                        1,
                        "",
                        "",
                    )
                },
                kind: nom::error::ErrorKind::Space,
                context: "".to_string(),
            })),

            // "KEYS IN", // 2 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::KeysIn),
            )),

            // "KEYS NOT IN", // 3 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[3].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::KeysIn),
            )),

            // "KEYS EXISTS", // 4 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::KeysExists),
            )),

            // "KEYS !EXISTS", // 5 Ok,
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::KeysExists),
            )),

            // "KEYS ==", // 6 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[6].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::KeysEq),
            )),

            // "KEYS !=", // 7 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[7].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::KeysEq),
            )),

            // "keys ! in", // 8 err after !
            Err(nom::Err::Error(ParserError {
                span: unsafe {
                    Span2::new_from_raw_offset(
                        "keys !".len(),
                        1,
                        " in",
                        "",
                    )
                },
                kind: nom::error::ErrorKind::Tag,
                context: "".to_string(),
            })),

            // "KEYS EMPTY", // 9 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[9].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::KeysEmpty),
            )),

            // "KEYS !EMPTY", // 10 ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[10].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::KeysEmpty),
            )),

            // " KEYS IN", // 11 err
            Err(nom::Err::Error(ParserError {
                span: from_str2(" KEYS IN"),
                kind: nom::error::ErrorKind::Tag,
                context: "".to_string(),
            })),

            // "KEYS ", // 12 err
            Err(nom::Err::Error(ParserError {
                span: unsafe {
                    Span2::new_from_raw_offset(
                        "KEYS ".len(),
                        1,
                        "",
                        "",
                    )
                },
                kind: nom::error::ErrorKind::Tag,
                context: "".to_string(),
            })),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = from_str2(*each);
            let result = keys_keyword(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }

    #[test]
    fn test_value_cmp() {
        let examples = [
            "", // err 0
            " >", // err 1,

            ">", // ok, 2
            ">=", // ok, 3
            "<", // ok, 4
            "<= ", // ok, 5
            ">=\n", // ok, 6
            "IN\n", // ok 7
            "!IN\n", // ok 8
        ];

        let expectations = [
            // "", // err 0
            Err(nom::Err::Error(ParserError {
                span: from_str2(examples[0]),
                context: "".to_string(),
                kind: nom::error::ErrorKind::Tag,
            })),

            // " >", // err 1,
            Err(nom::Err::Error(ParserError {
                span: from_str2(examples[1]),
                context: "".to_string(),
                kind: nom::error::ErrorKind::Tag,
            })),


            // ">", // ok, 2
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Gt)
            )),

            // ">=", // ok, 3
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[3].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Ge)
            )),

            // "<", // ok, 4
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        1,
                        "",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Lt)
            )),

            // "<= ", // ok, 5
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len() - 1,
                        1,
                        " ",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Le)
            )),

            // ">=\n", // ok, 6
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[6].len() - 1,
                        1,
                        "\n",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::Ge)
            )),

            // "IN\n", // ok 7
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[7].len() - 1,
                        1,
                        "\n",
                        "",
                    )
                },
                ValueOperator::Cmp(CmpOperator::In)
            )),

            // "!IN\n", // ok 8
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[8].len() - 1,
                        1,
                        "\n",
                        "",
                    )
                },
                ValueOperator::Not(CmpOperator::In)
            )),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = from_str2(*each);
            let result = value_cmp(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }

    #[test]
    fn test_clause_success() {
        let lhs = [
            "configuration.containers.*.image",
            "engine",
        ];

        let rhs = "PARAMETERS.ImageList";
        let comparators = [
            (">", ValueOperator::Cmp(CmpOperator::Gt)),
            ("<", ValueOperator::Cmp(CmpOperator::Lt)),
            ("==", ValueOperator::Cmp(CmpOperator::Eq)),
            ("!=", ValueOperator::Not(CmpOperator::Eq)),
            ("IN", ValueOperator::Cmp(CmpOperator::In)),
            ("!IN", ValueOperator::Not(CmpOperator::In)),
            ("not IN", ValueOperator::Not(CmpOperator::In)),
            ("NOT IN", ValueOperator::Not(CmpOperator::In)),
            ("KEYS IN", ValueOperator::Cmp(CmpOperator::KeysIn)),
            ("KEYS ==", ValueOperator::Cmp(CmpOperator::KeysEq)),
            ("KEYS !=", ValueOperator::Not(CmpOperator::KeysEq)),
            ("KEYS !IN", ValueOperator::Not(CmpOperator::KeysIn)),
        ];
        let separators = [
            (" ", " "),
            ("\t", "\n\n\t"),
            ("\t  ", "\t\t"),
            (" ", "\n#this comment\n"),
            (" ", "#this comment\n")
        ];

        let rhs_dotted = rhs.split(".").map(String::from).collect::<Vec<String>>();
        let rhs_access = Some(LetValue::PropertyAccess(PropertyAccess {
            var_access: None,
            property_dotted_notation: rhs_dotted,
        }));

        for each_lhs in lhs.iter() {
            let dotted = (*each_lhs).split(".").map(String::from).collect::<Vec<String>>();
            let lhs_access = PropertyAccess {
                var_access: None,
                property_dotted_notation: dotted,
            };

            testing_access_with_cmp(&separators, &comparators,
                                    *each_lhs, rhs,
                                    || lhs_access.clone(),
                                    || rhs_access.clone());
        }

        let comparators = [
            ("EXISTS", ValueOperator::Cmp(CmpOperator::Exists)),
            ("!EXISTS", ValueOperator::Not(CmpOperator::Exists)),
            ("EMPTY", ValueOperator::Cmp(CmpOperator::Empty)),
            ("NOT EMPTY", ValueOperator::Not(CmpOperator::Empty)),
            ("KEYS EXISTS", ValueOperator::Cmp(CmpOperator::KeysExists)),
            ("KEYS NOT EMPTY", ValueOperator::Not(CmpOperator::KeysEmpty))
        ];

        for each_lhs in lhs.iter() {
            let dotted = (*each_lhs).split(".").map(String::from).collect::<Vec<String>>();
            let lhs_access = PropertyAccess {
                var_access: None,
                property_dotted_notation: dotted,
            };

            testing_access_with_cmp(&separators, &comparators,
                                    *each_lhs, "",
                                    || lhs_access.clone(),
                                    || None);
        }

        for each_lhs in lhs.iter() {
            let dotted = (*each_lhs).split(".").map(String::from).collect::<Vec<String>>();
            let lhs_access = PropertyAccess {
                var_access: None,
                property_dotted_notation: dotted,
            };

            testing_access_with_cmp(&separators, &comparators,
                                    *each_lhs, " does.not.error", // this will not error,
                                    // the fragment you are left with is the one above and
                                    // the next clause fetch will error out for either no "OR" or
                                    // not newline for "and"
                                    || lhs_access.clone(),
                                    || None);
        }


        let lhs = [
            "%engine.port",
            "%engine.%port",
            "%engine.*.image"
        ];

        for each_lhs in lhs.iter() {
            let dotted = (*each_lhs).split(".").map(String::from).collect::<Vec<String>>();
            let (var_name, remainder) = dotted.split_at(1);
            let dotted = remainder.iter().map(|s| s.to_owned())
                .collect::<Vec<String>>();
            let var_name = var_name[0].replace("%", "");
            let lhs_access = PropertyAccess {
                var_access: Some(var_name),
                property_dotted_notation: dotted,
            };

            testing_access_with_cmp(&separators, &comparators,
                                    *each_lhs, "",
                                    || lhs_access.clone(),
                                    || None);
        }

        let rhs = [
            "\"ami-12344545\"",
            "/ami-12/",
            "[\"ami-12\", \"ami-21\"]",
            "{ bare: 10, 'work': 20, 'other': 12.4 }"
        ];
        let comparators = [
            (">", ValueOperator::Cmp(CmpOperator::Gt)),
            ("<", ValueOperator::Cmp(CmpOperator::Lt)),
            ("==", ValueOperator::Cmp(CmpOperator::Eq)),
            ("!=", ValueOperator::Not(CmpOperator::Eq)),
            ("IN", ValueOperator::Cmp(CmpOperator::In)),
            ("!IN", ValueOperator::Not(CmpOperator::In)),
        ];

        for each_rhs in &rhs {
            for each_lhs in lhs.iter() {
                let dotted = (*each_lhs).split(".").map(String::from).collect::<Vec<String>>();
                let (var_name, remainder) = dotted.split_at(1);
                let dotted = remainder.iter().map(|s| s.to_owned())
                    .collect::<Vec<String>>();
                let var_name = var_name[0].replace("%", "");
                let lhs_access = PropertyAccess {
                    var_access: Some(var_name),
                    property_dotted_notation: dotted,
                };

                let rhs_value = parse_value(from_str2(*each_rhs)).unwrap().1;
                testing_access_with_cmp(&separators, &comparators,
                                        *each_lhs, *each_rhs,
                                        || lhs_access.clone(),
                                        || Some(LetValue::Value(rhs_value.clone())));
            }
        }
    }

    fn testing_access_with_cmp<A, C>(separators: &[(&str, &str)],
                                     comparators: &[(&str, ValueOperator)],
                                     lhs: &str,
                                     rhs: &str,
                                     access: A,
                                     cmp_with: C)
        where A: Fn() -> PropertyAccess,
              C: Fn() -> Option<LetValue>
    {
        for (lhs_sep, rhs_sep) in separators {
            for (_idx, (each_op, value_cmp)) in comparators.iter().enumerate() {
                let access_pattern = format!("{lhs}{lhs_sep}{op}{rhs_sep}{rhs}",
                                             lhs = lhs, rhs = rhs, op = *each_op, lhs_sep = *lhs_sep, rhs_sep = *rhs_sep);
                println!("Testing Access pattern = {}", access_pattern);
                let span = from_str2(&access_pattern);
                let result = clause(span);
                if result.is_err() {
                    let parser_error = &result.unwrap_err();
                    let parser_error = match parser_error {
                        nom::Err::Error(p) | nom::Err::Failure(p) => format!("ParserError = {} fragment = {}", p, *p.span.fragment()),
                        nom::Err::Incomplete(_) => "More input needed".to_string(),
                    };
                    println!("{}", parser_error);
                    assert_eq!(false, true);
                } else {
                    assert_eq!(result.is_ok(), true);
                    let result = match result.unwrap().1 {
                        GuardClause::Clause(clause, _) => clause,
                        _ => unreachable!()
                    };
                    assert_eq!(result.access, access());
                    assert_eq!(result.compare_with, cmp_with());
                    assert_eq!(&result.comparator, value_cmp);
                    assert_eq!(result.custom_message, None);
                }
            }
        }
    }

    #[test]
    fn test_clause_failures() {
        let lhs = [
            "configuration.containers.*.image",
            "engine",
        ];

        //
        // Testing white space problems
        //
        let rhs = "PARAMETERS.ImageList";
        let lhs_separator = "";
        let rhs_separator = "";
        let comparators = [
            (">", ValueOperator::Cmp(CmpOperator::Gt)),
            ("<", ValueOperator::Cmp(CmpOperator::Lt)),
            ("==", ValueOperator::Cmp(CmpOperator::Eq)),
            ("!=", ValueOperator::Not(CmpOperator::Eq)),
        ];

        for each in lhs.iter() {
            for (op, _) in comparators.iter() {
                let access_pattern = format!("{lhs}{lhs_sep}{op}{rhs_sep}{rhs}",
                                             lhs = *each, rhs = rhs, op = *op, lhs_sep = lhs_separator, rhs_sep = rhs_separator);
                let offset = (*each).len();
                let fragment = format!("{op}{sep}{rhs}",
                                       rhs = rhs, op = *op, sep = rhs_separator);
                let error = Err(nom::Err::Error(ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            offset,
                            1,
                            &fragment,
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Space,
                    context: "expecting one or more WS or comment blocks".to_string(),
                }));
                assert_eq!(clause(from_str2(&access_pattern)), error);
            }
        }

        let lhs_separator = " ";
        for each in lhs.iter() {
            for (op, _) in comparators.iter() {
                let access_pattern = format!("{lhs}{lhs_sep}{op}{rhs_sep}{rhs}{msg}",
                                             lhs = *each, rhs = rhs, op = *op, lhs_sep = lhs_separator, rhs_sep = rhs_separator, msg = "<< message >>");
                let offset = (*each).len() + (*op).len() + 1;
                let fragment = format!("{sep}{rhs}{msg}", rhs = rhs, sep = rhs_separator, msg = "<< message >>");
                let error = Err(nom::Err::Error(ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            offset,
                            1,
                            &fragment,
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Char,
                    context: "expecting one or more WS or comment blocks".to_string(),
                }));
                assert_eq!(clause(from_str2(&access_pattern)), error);
            }
        }

        //
        // Testing for missing access part
        //
        assert_eq!(Err(nom::Err::Error(ParserError {
            span: from_str2(""),
            kind: nom::error::ErrorKind::Alpha,
            context: "".to_string(),
        })), clause(from_str2("")));

        //
        // Testing for missing access
        //
        assert_eq!(Err(nom::Err::Error(ParserError {
            span: from_str2(" > 10"),
            kind: nom::error::ErrorKind::Alpha,
            context: "".to_string(),
        })), clause(from_str2(" > 10")));

        //
        // Testing binary operator missing RHS
        //
        for each in lhs.iter() {
            for (op, _) in comparators.iter() {
                let access_pattern = format!("{lhs} {op} << message >>", lhs = *each, op = *op);
                println!("Testing for {}", access_pattern);
                let offset = (*each).len() + (*op).len() + 2; // 2 is for 2 spaces
                let error = Err(nom::Err::Failure(ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            offset,
                            1,
                            "<< message >>",
                            "",
                        )
                    },
                    kind: nom::error::ErrorKind::Char, // this comes off parse_map
                    context: r#"expecting either a property access "engine.core" or value like "string" or ["this", "that"]"#.to_string(),
                }));
                assert_eq!(clause(from_str2(&access_pattern)), error);
            }
        }
    }

    #[test]
    fn test_rule_clauses() {
        let examples = [
            "",                             // 0 err
            "secure\n",                     // 1 Ok
            "!secure or !encrypted",        // 2 Ok
            "secure\n\nor\t encrypted",     // 3 Ok
            "let x = 10",                   // 4 err
            "port == 10",                   // 5 err
            "secure <<this is secure ${PARAMETER.MSG}>>", // 6 Ok
            "!secure <<this is not secure ${PARAMETER.MSG}>> or !encrypted", // 7 Ok
        ];

        let expectations = [
            // "",                             // 0 err
            Err(nom::Err::Error(ParserError {
                span: from_str2(""),
                kind: nom::error::ErrorKind::Alpha,
                context: "".to_string(),
            })),

            // "secure",                       // 1 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[1].len() - 1,
                        1,
                        "\n",
                        ""
                    )
                },
                GuardClause::NamedRule(
                    "secure".to_string(),
                    Location { line: 1, column: 1, file_name: "" },
                    false,
                    None)
            )),

            // "!secure or !encrypted",        // 2 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        "!secure".len(),
                        1,
                        " or !encrypted",
                        ""
                    )
                },
                GuardClause::NamedRule(
                    "secure".to_string(),
                    Location { line: 1, column: 1, file_name: "" },
                    true,
                    None)
            )),

            // "secure\n\nor\t encrypted",     // 3 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        "secure".len(),
                        1,
                        "\n\nor\t encrypted",
                        ""
                    )
                },
                GuardClause::NamedRule(
                    "secure".to_string(),
                    Location { line: 1, column: 1, file_name: "" },
                    false,
                    None)
            )),

            // "let x = 10",                   // 4 err
            Err(nom::Err::Error(
                ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            "let ".len(),
                        1,
                            "x = 10",
                            ""
                        )
                    },
                    kind: nom::error::ErrorKind::Tag,
                    context: "".to_string(),
                }
            )),

            // "port == 10",                   // 5 err
            Err(nom::Err::Error(
                ParserError {
                    span: unsafe {
                        Span2::new_from_raw_offset(
                            "port ".len(),
                            1,
                            "== 10",
                            ""
                        )
                    },
                    kind: nom::error::ErrorKind::Tag,
                    context: "".to_string(),
                }
            )),

            // "secure <<this is secure ${PARAMETER.MSG}>>", // 6 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[6].len(),
                        1,
                        "",
                        "",
                    )
                },
                GuardClause::NamedRule(
                    "secure".to_string(),
                    Location { line: 1, column: 1, file_name: "" },
                    false,
                    Some("this is secure ${PARAMETER.MSG}".to_string())),
            )),

            // "!secure <<this is not secure ${PARAMETER.MSG}>> or !encrypted" // 8 Ok
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[7].len() - " or !encrypted".len(),
                        1,
                        " or !encrypted",
                        ""
                    )
                },
                GuardClause::NamedRule(
                    "secure".to_string(),
                    Location { line: 1, column: 1, file_name: "" },
                    true,
                    Some("this is not secure ${PARAMETER.MSG}".to_string())),
            )),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = from_str2(*each);
            let result = rule_clause(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }

    #[test]
    fn test_clauses() {
        let examples = [
            "", // Ok 0
            "secure\n", // Ok 1
            "!secure << was not secure ${PARAMETER.SECURE_MSG}>>", // Ok 2
            "secure\nconfigurations.containers.*.image == /httpd:2.4/", // Ok 3
            r#"secure or
               !exception

               configurations.containers.*.image == /httpd:2.4/"#, // Ok 4
            r#"secure or
               !exception
               let x = 10"# // Ok 5
        ];

        let expectations = [

            // "", // err 0
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[0].len(),
                        1,
                        "",
                        "",
                    )
                },
                vec![],
            )),

            // "secure\n", // Ok 1
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[1].len() - 1,
                        1,
                        "\n",
                        "",
                    )
                },
                vec![
                    ConjunctionClause::And(
                        GuardClause::NamedRule(
                            "secure".to_string(),
                            Location {
                                line: 1,
                                column: 1,
                                file_name: "",
                            },
                            false,
                            None,
                        )
                    )
                ]
            )),

            // "!secure << was not secure ${PARAMETER.SECURE_MSG}>>", // Ok 2
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[2].len(),
                        1,
                        "",
                        "",
                    )
                },
                vec![ConjunctionClause::And(
                    GuardClause::NamedRule(
                        "secure".to_string(),
                        Location {
                            line: 1,
                            column: 1,
                            file_name: "",
                        },
                        true,
                        Some(" was not secure ${PARAMETER.SECURE_MSG}".to_string())))]
            )),

            // "secure\nconfigurations.containers.*.image == /httpd:2.4/", // Ok 3
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[3].len(),
                        2,
                        "",
                        "",
                    )
                },
                vec![
                    ConjunctionClause::And(
                        GuardClause::NamedRule(
                            "secure".to_string(),
                            Location {
                                line: 1,
                                column: 1,
                                file_name: "",
                            },
                            false,
                            None)),
                    ConjunctionClause::And(
                        GuardClause::Clause(
                            Clause {
                                location: Location {
                                    file_name: "",
                                    column: 1,
                                    line: 2,
                                },
                                compare_with: Some(LetValue::Value(Value::Regex("httpd:2.4".to_string()))),
                                access: PropertyAccess {
                                    var_access: None,
                                    property_dotted_notation: "configurations.containers.*.image".split(".").map(String::from).collect(),
                                },
                                custom_message: None,
                                comparator: ValueOperator::Cmp(CmpOperator::Eq),
                            },
                            false,
                        )
                    )]
            )),

            // r#"secure or
            //    !exception
            //
            //    configurations.containers.*.image == /httpd:2.4/"#, // Ok 4
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[4].len(),
                        4,
                        "",
                        "",
                    )
                },
                vec![
                    ConjunctionClause::Or(
                        vec![
                            GuardClause::NamedRule("secure".to_string(), Location { line: 1, column: 1, file_name: "" }, false, None),
                            GuardClause::NamedRule("exception".to_string(), Location { line: 2, column: 16, file_name: "" }, true, None),
                        ],
                        false,
                    ),
                    ConjunctionClause::And(
                        GuardClause::Clause(
                            Clause {
                                location: Location { file_name: "", column: 16, line: 4 },
                                compare_with: Some(LetValue::Value(Value::Regex("httpd:2.4".to_string()))),
                                access: PropertyAccess {
                                    var_access: None,
                                    property_dotted_notation: "configurations.containers.*.image".split(".").map(String::from).collect(),
                                },
                                custom_message: None,
                                comparator: ValueOperator::Cmp(CmpOperator::Eq),
                            },
                            false,
                        )
                    ),
                ]
            )),

            // r#"secure or
            //    !exception
            //    let x = 10"# // Ok 5
            Ok((
                unsafe {
                    Span2::new_from_raw_offset(
                        examples[5].len() - "\n               let x = 10".len(),
                        2,
                        "\n               let x = 10",
                        "",
                    )
                },
                vec![
                    ConjunctionClause::Or(
                        vec![
                            GuardClause::NamedRule("secure".to_string(), Location { line: 1, column: 1, file_name: "" }, false, None),
                            GuardClause::NamedRule("exception".to_string(), Location { line: 2, column: 16, file_name: "" }, true, None),
                        ],
                        false,
                    ),
                ]
            )),
        ];

        for (idx, each) in examples.iter().enumerate() {
            let span = from_str2(*each);
            let result = clauses(span);
            assert_eq!(&result, &expectations[idx]);
        }
    }
}
