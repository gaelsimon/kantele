//! The `Search` criteria language.

use std::fmt;

use crate::index::fold;
use crate::index::searchable::{Field, Fields};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Class,
    Title,
    Creator,
    Artist(Option<Role>),
    Album,
    Genre,
    Date,
    /// Nothing served is a reference, so only `exists false` matches.
    RefId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    AlbumArtist,
    Composer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Equals,
    NotEquals,
    Contains,
    DoesNotContain,
    DerivedFrom,
    Exists,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Criterion {
    pub property: Property,
    pub operator: Operator,
    pub operand: String,
    folded: String,
}

impl Criterion {
    pub fn new(property: Property, operator: Operator, operand: impl Into<String>) -> Self {
        let operand = operand.into();
        Self {
            property,
            operator,
            folded: fold(&operand),
            operand,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Criteria {
    Everything,
    Expression(Node),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    One(Criterion),
    /// Never shorter than two.
    All(Vec<Node>),
    /// Never shorter than two.
    Any(Vec<Node>),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("empty search criteria")]
    Empty,
    #[error("incorrect search query: 'and' and 'or' at one level need parentheses")]
    Mixed,
    #[error("unsupported search query property {0:?}")]
    UnknownProperty(String),
    #[error("unsupported search operator {0:?}")]
    UnknownOperator(String),
    #[error("incorrect search query: {0}")]
    Malformed(&'static str),
    #[error("unterminated quoted string")]
    UnterminatedQuote,
}

impl fmt::Display for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Class => "upnp:class",
            Self::Title => "dc:title",
            Self::Creator => "dc:creator",
            Self::Artist(None) => "upnp:artist",
            Self::Artist(Some(Role::AlbumArtist)) => r#"upnp:artist[@role="AlbumArtist"]"#,
            Self::Artist(Some(Role::Composer)) => r#"upnp:artist[@role="Composer"]"#,
            Self::Album => "upnp:album",
            Self::Genre => "upnp:genre",
            Self::Date => "dc:date",
            Self::RefId => "@refID",
        })
    }
}

pub const EVALUATED: &[Property] = &[
    Property::Class,
    Property::Title,
    Property::Creator,
    Property::Artist(None),
    Property::Artist(Some(Role::AlbumArtist)),
    Property::Artist(Some(Role::Composer)),
    Property::Album,
    Property::Genre,
    Property::Date,
    Property::RefId,
];

pub fn capabilities() -> String {
    EVALUATED
        .iter()
        .map(Property::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

const DEEPEST: usize = 32;

pub fn parse(criteria: &str) -> Result<Criteria, ParseError> {
    let tokens = tokenize(criteria)?;
    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }
    if tokens.len() == 1 && tokens[0] == Token::Star {
        return Ok(Criteria::Everything);
    }
    let (node, rest) = expression(&tokens, 0)?;
    if !rest.is_empty() {
        return Err(ParseError::Malformed("unexpected text after the criteria"));
    }
    Ok(Criteria::Expression(node))
}

fn joiner(token: Option<&Token>) -> Option<bool> {
    let Token::Word(word) = token? else {
        return None;
    };
    match word {
        _ if word.eq_ignore_ascii_case("and") => Some(true),
        _ if word.eq_ignore_ascii_case("or") => Some(false),
        _ => None,
    }
}

fn expression(tokens: &[Token], depth: usize) -> Result<(Node, &[Token]), ParseError> {
    let (first, mut rest) = term(tokens, depth)?;
    let mut nodes = vec![first];
    let mut held: Option<bool> = None;
    while let Some(next) = joiner(rest.first()) {
        match held {
            None => held = Some(next),
            Some(already) if already != next => return Err(ParseError::Mixed),
            Some(_) => {}
        }
        let (node, tail) = term(&rest[1..], depth)?;
        nodes.push(node);
        rest = tail;
    }
    let node = match held {
        None => nodes.pop().expect("a run holds its first term"),
        Some(true) => Node::All(nodes),
        Some(false) => Node::Any(nodes),
    };
    Ok((node, rest))
}

fn term(tokens: &[Token], depth: usize) -> Result<(Node, &[Token]), ParseError> {
    if tokens.first() == Some(&Token::Open) {
        if depth >= DEEPEST {
            return Err(ParseError::Malformed("criteria nested too deeply"));
        }
        let (node, rest) = expression(&tokens[1..], depth + 1)?;
        let [Token::Close, tail @ ..] = rest else {
            return Err(ParseError::Malformed("expected a closing parenthesis"));
        };
        return Ok((node, tail));
    }
    let (criterion, rest) = criterion(tokens)?;
    Ok((Node::One(criterion), rest))
}

fn criterion(tokens: &[Token]) -> Result<(Criterion, &[Token]), ParseError> {
    let [Token::Word(name), Token::Word(operator), rest @ ..] = tokens else {
        return Err(ParseError::Malformed("expected a property and an operator"));
    };
    let property = property(name)?;
    let operator = operator_of(operator)?;

    let (operand, tail) = match rest.first() {
        Some(Token::Quoted(value)) => (value.clone(), &rest[1..]),
        Some(Token::Word(value)) if operator == Operator::Exists => (value.clone(), &rest[1..]),
        _ => return Err(ParseError::Malformed("expected an operand")),
    };
    if operator == Operator::Exists && !matches!(operand.as_str(), "true" | "false") {
        return Err(ParseError::Malformed("exists takes true or false"));
    }
    Ok((Criterion::new(property, operator, operand), tail))
}

fn property(name: &str) -> Result<Property, ParseError> {
    for known in EVALUATED {
        if known.to_string().eq_ignore_ascii_case(name) {
            return Ok(*known);
        }
    }
    Err(ParseError::UnknownProperty(name.to_owned()))
}

fn operator_of(word: &str) -> Result<Operator, ParseError> {
    Ok(match word.to_ascii_lowercase().as_str() {
        "=" => Operator::Equals,
        "!=" => Operator::NotEquals,
        "contains" => Operator::Contains,
        "doesnotcontain" => Operator::DoesNotContain,
        "derivedfrom" => Operator::DerivedFrom,
        "exists" => Operator::Exists,
        _ => return Err(ParseError::UnknownOperator(word.to_owned())),
    })
}

fn fields_of(property: Property) -> &'static [Field] {
    match property {
        Property::Title => &[Field::Title],
        Property::Creator => &[Field::Creator],
        Property::Artist(None) => &[Field::Artists, Field::AlbumArtists],
        Property::Artist(Some(Role::AlbumArtist)) => &[Field::AlbumArtists],
        Property::Artist(Some(Role::Composer)) => &[Field::Composers],
        Property::Album => &[Field::Album],
        Property::Genre => &[Field::Genres],
        Property::Date => &[Field::Date],
        Property::Class | Property::RefId => &[],
    }
}

impl Criteria {
    pub fn matches(&self, fields: &impl Fields) -> bool {
        match self {
            Self::Everything => true,
            Self::Expression(node) => node.matches(fields),
        }
    }
}

impl Node {
    pub fn matches(&self, fields: &impl Fields) -> bool {
        match self {
            Self::One(criterion) => criterion.matches(fields),
            Self::All(nodes) => nodes.iter().all(|node| node.matches(fields)),
            Self::Any(nodes) => nodes.iter().any(|node| node.matches(fields)),
        }
    }
}

impl Criterion {
    pub fn matches(&self, fields: &impl Fields) -> bool {
        if self.property == Property::RefId {
            return self.operand == "false";
        }
        if self.property == Property::Class {
            let class = fields.class();
            return match self.operator {
                Operator::Equals => class == self.folded,
                Operator::Contains => class.contains(&self.folded),
                Operator::DerivedFrom => derived_from(class, &self.folded),
                Operator::NotEquals => class != self.folded,
                Operator::DoesNotContain => !class.contains(&self.folded),
                Operator::Exists => self.operand == "true",
            };
        }
        let read = fields_of(self.property);
        let any =
            |test: &mut dyn FnMut(&str) -> bool| read.iter().any(|field| fields.any(*field, test));
        match self.operator {
            Operator::Equals => any(&mut |value| value == self.folded),
            Operator::Contains => any(&mut |value| value.contains(&self.folded)),
            Operator::DerivedFrom => any(&mut |value| derived_from(value, &self.folded)),
            Operator::NotEquals => !any(&mut |value| value == self.folded),
            Operator::DoesNotContain => !any(&mut |value| value.contains(&self.folded)),
            Operator::Exists => {
                read.iter().any(|field| fields.present(*field)) == (self.operand == "true")
            }
        }
    }
}

fn derived_from(class: &str, base: &str) -> bool {
    class == base
        || (class.len() > base.len()
            && class.starts_with(base)
            && class.as_bytes()[base.len()] == b'.')
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    Quoted(String),
    Open,
    Close,
    Star,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            '*' if tokens.is_empty() => tokens.push(Token::Star),
            '"' => tokens.push(Token::Quoted(quoted(&mut chars)?)),
            _ => tokens.push(Token::Word(word(input, start, c, &mut chars))),
        }
    }
    Ok(tokens)
}

fn quoted(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, ParseError> {
    let mut value = String::new();
    while let Some((_, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, escaped)) = chars.next() {
                    value.push(escaped);
                }
            }
            '"' => return Ok(value),
            c => value.push(c),
        }
    }
    Err(ParseError::UnterminatedQuote)
}

fn word(
    input: &str,
    start: usize,
    first: char,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> String {
    let mut end = start + first.len_utf8();
    let mut depth = 0usize;
    while let Some((at, c)) = chars.peek().copied() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            c if depth == 0 && (c.is_whitespace() || c == '(' || c == ')') => break,
            _ => {}
        }
        end = at + c.len_utf8();
        chars.next();
    }
    input[start..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::credits::Credit;
    use crate::index::searchable::{self, Raw};

    fn one(criteria: &str) -> Criterion {
        match parse(criteria).expect("it parses") {
            Criteria::Expression(Node::One(criterion)) => criterion,
            other => panic!("expected one criterion, got {other:?}"),
        }
    }

    fn all(criteria: Vec<Node>) -> Criteria {
        Criteria::Expression(Node::All(criteria))
    }

    fn of(property: Property, operator: Operator, operand: &str) -> Node {
        Node::One(Criterion::new(property, operator, operand))
    }

    #[test]
    fn what_an_amplifier_menu_actually_sends() {
        let parsed =
            parse(r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#)
                .expect("it parses");
        assert_eq!(
            parsed,
            all(vec![
                of(
                    Property::Class,
                    Operator::DerivedFrom,
                    "object.container.album"
                ),
                of(Property::RefId, Operator::Exists, "false"),
            ])
        );
    }

    #[test]
    fn the_bracketed_role_is_one_property_and_not_a_quoted_string() {
        let criterion = one(r#"upnp:artist[@role="Composer"] contains "beethoven""#);
        assert_eq!(criterion.property, Property::Artist(Some(Role::Composer)));
        assert_eq!(criterion.operand, "beethoven");
    }

    #[test]
    fn the_unbracketed_role_is_an_unknown_property_rather_than_a_role() {
        assert!(matches!(
            parse(r#"upnp:artist@role = "Composer""#),
            Err(ParseError::UnknownProperty(_))
        ));
    }

    #[test]
    fn a_disjunction_is_evaluated_and_so_are_the_parentheses_around_it() {
        assert_eq!(
            parse(r#"dc:title = "a" or dc:title = "b""#).expect("or is evaluated"),
            Criteria::Expression(Node::Any(vec![
                of(Property::Title, Operator::Equals, "a"),
                of(Property::Title, Operator::Equals, "b"),
            ]))
        );
        assert_eq!(
            parse(r#"(upnp:class = "object.item") and (dc:title contains "x")"#)
                .expect("parentheses are accepted"),
            all(vec![
                of(Property::Class, Operator::Equals, "object.item"),
                of(Property::Title, Operator::Contains, "x"),
            ])
        );
    }

    #[test]
    fn a_disjunction_nested_inside_a_conjunction_is_the_shape_a_control_point_sends() {
        let parsed = parse(
            r#"upnp:class derivedfrom "object.item.audioItem" and (dc:creator contains "soul" or upnp:artist contains "soul")"#,
        )
        .expect("it parses");
        assert_eq!(
            parsed,
            all(vec![
                of(
                    Property::Class,
                    Operator::DerivedFrom,
                    "object.item.audioItem"
                ),
                Node::Any(vec![
                    of(Property::Creator, Operator::Contains, "soul"),
                    of(Property::Artist(None), Operator::Contains, "soul"),
                ]),
            ])
        );
    }

    #[test]
    fn and_and_or_at_one_level_are_refused_because_the_grammar_gives_them_no_precedence() {
        assert_eq!(
            parse(r#"dc:title = "a" and dc:title = "b" or dc:title = "c""#),
            Err(ParseError::Mixed)
        );
        assert_eq!(
            parse(r#"dc:title = "a" or dc:title = "b" and dc:title = "c""#),
            Err(ParseError::Mixed)
        );
        assert!(
            parse(r#"dc:title = "a" and (dc:title = "b" or dc:title = "c")"#).is_ok(),
            "parentheses say what the mixture meant"
        );
    }

    #[test]
    fn an_unclosed_or_nonsense_parenthesis_is_refused_rather_than_read_past() {
        for criteria in [
            r#"(dc:title = "a""#,
            r#"dc:title = "a")"#,
            "()",
            r#"(dc:title = "a" or)"#,
        ] {
            assert!(parse(criteria).is_err(), "{criteria} should not parse");
        }
    }

    #[test]
    fn a_criteria_string_cannot_nest_deeply_enough_to_exhaust_the_stack() {
        let deep = format!(
            "{}dc:title = \"a\"{}",
            "(".repeat(DEEPEST + 2),
            ")".repeat(DEEPEST + 2)
        );
        assert!(matches!(parse(&deep), Err(ParseError::Malformed(_))));
    }

    #[test]
    fn exists_takes_its_boolean_bare_or_quoted() {
        assert_eq!(one("@refID exists false").operand, "false");
        assert_eq!(one(r#"@refID exists "false""#).operand, "false");
        assert!(matches!(
            parse("@refID exists maybe"),
            Err(ParseError::Malformed(_))
        ));
    }

    #[test]
    fn a_title_containing_the_word_and_survives() {
        let criterion = one(r#"dc:title contains "rhythm and blues""#);
        assert_eq!(criterion.operand, "rhythm and blues");
    }

    #[test]
    fn a_quote_inside_a_title_is_escaped_rather_than_ending_it() {
        assert_eq!(one(r#"dc:title = "12\" mix""#).operand, "12\" mix");
    }

    #[test]
    fn the_star_matches_everything() {
        assert_eq!(parse("*").expect("it parses"), Criteria::Everything);
        assert_eq!(parse("  *  ").expect("it parses"), Criteria::Everything);
    }

    #[test]
    fn an_empty_criteria_string_is_a_fault_rather_than_everything() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(parse("   "), Err(ParseError::Empty));
    }

    #[test]
    fn an_unknown_property_faults_rather_than_returning_the_whole_library() {
        assert_eq!(
            parse(r#"kantele:nonsense contains "x""#),
            Err(ParseError::UnknownProperty("kantele:nonsense".to_owned()))
        );
    }

    #[test]
    fn an_operator_with_no_operand_is_a_fault() {
        assert!(matches!(
            parse("upnp:class derivedfrom"),
            Err(ParseError::Malformed(_))
        ));
    }

    #[test]
    fn an_unterminated_quote_is_a_fault_rather_than_a_silent_truncation() {
        assert_eq!(
            parse(r#"dc:title contains "unfinished"#),
            Err(ParseError::UnterminatedQuote)
        );
    }

    #[test]
    fn operators_and_properties_are_case_insensitive() {
        assert_eq!(
            one(r#"UPNP:CLASS DERIVEDFROM "object.item""#).property,
            Property::Class
        );
        assert_eq!(
            one(r#"upnp:class derivedFrom "object.item""#).operator,
            Operator::DerivedFrom
        );
    }

    #[test]
    fn every_declared_capability_parses() {
        for property in EVALUATED {
            let criteria = match property {
                Property::RefId => format!("{property} exists false"),
                other => format!(r#"{other} contains "x""#),
            };
            assert_eq!(
                one(&criteria).property,
                *property,
                "{property} is declared and does not parse"
            );
        }
    }

    const TRACK: &str = "object.item.audioItem.musicTrack";

    fn subject(class: &str, raw: Raw<'_>) -> searchable::Folded {
        searchable::Folded::of(class, raw)
    }

    fn track() -> searchable::Folded {
        subject(
            TRACK,
            Raw {
                title: "No Me Llores Más",
                ..Raw::default()
            },
        )
    }

    #[test]
    fn a_title_search_reads_the_title_and_nothing_else_whoever_is_asked() {
        let artists = vec![Credit::new("La Troba Kung Fu")];
        let raw = |title| Raw {
            title,
            artists: &artists,
            ..Raw::default()
        };
        let contains = |object: &searchable::Folded, term: &str| {
            parse(&format!(r#"dc:title contains "{term}""#))
                .expect("it parses")
                .matches(object)
        };

        let album = subject("object.container.album.musicAlbum", raw("Cumbia Inferno"));
        assert!(!contains(&album, "kung fu"));
        assert!(!contains(&subject(TRACK, raw("Barcelona")), "kung fu"));
        assert!(contains(
            &subject(TRACK, raw("Kung Fu Fighting")),
            "kung fu"
        ));

        let artist = subject(
            "object.container.person.musicArtist",
            Raw {
                title: "La Troba Kung Fu",
                ..Raw::default()
            },
        );
        assert!(contains(&artist, "kung fu"));
    }

    #[test]
    fn derived_from_matches_the_base_class_and_not_a_longer_word() {
        assert!(derived_from(
            "object.container.album.musicAlbum",
            "object.container.album"
        ));
        assert!(derived_from(
            "object.container.album",
            "object.container.album"
        ));
        assert!(!derived_from(
            "object.container.albumOther",
            "object.container.album"
        ));
        assert!(!derived_from("object.container", "object.container.album"));
    }

    #[test]
    fn an_amplifier_asking_for_albums_gets_album_containers_and_not_tracks() {
        let criteria =
            parse(r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#)
                .expect("it parses");
        let album = subject(
            "object.container.album.musicAlbum",
            Raw {
                title: "!Dundunbanza!",
                ..Raw::default()
            },
        );
        assert!(criteria.matches(&album));
        assert!(!criteria.matches(&track()));
    }

    #[test]
    fn an_accent_is_folded_where_the_incumbent_only_folds_case() {
        let track = track();
        let criteria = parse(r#"dc:title contains "llores mas""#).expect("it parses");
        assert!(criteria.matches(&track));
        let upper = parse(r#"dc:title contains "LLORES MÁS""#).expect("it parses");
        assert!(upper.matches(&track));
    }

    #[test]
    fn contains_matches_inside_a_value_and_not_only_at_its_start() {
        let names = vec![Credit::new("Christian Thielemann, Wiener Philharmoniker")];
        let object = subject(
            TRACK,
            Raw {
                artists: &names,
                ..Raw::default()
            },
        );
        assert!(
            parse(r#"upnp:artist contains "thielemann""#)
                .expect("it parses")
                .matches(&object)
        );
    }

    #[test]
    fn a_role_narrows_rather_than_being_ignored() {
        let conductors = vec![Credit::new("Christian Thielemann")];
        let composers = vec![Credit::new("Ludwig van Beethoven")];
        let object = subject(
            TRACK,
            Raw {
                artists: &conductors,
                composers: &composers,
                ..Raw::default()
            },
        );
        let as_composer = |name| {
            parse(&format!(
                r#"upnp:artist[@role="Composer"] contains "{name}""#
            ))
            .expect("it parses")
            .matches(&object)
        };
        assert!(as_composer("beethoven"));
        assert!(!as_composer("thielemann"));
        assert!(
            parse(r#"upnp:artist contains "thielemann""#)
                .expect("it parses")
                .matches(&object)
        );
    }

    #[test]
    fn the_bare_property_does_not_reach_a_composer() {
        let composers = vec![Credit::new("Ludwig van Beethoven")];
        let object = subject(
            TRACK,
            Raw {
                composers: &composers,
                ..Raw::default()
            },
        );
        assert!(
            !parse(r#"upnp:artist contains "beethoven""#)
                .expect("it parses")
                .matches(&object),
            "a composer is not an artist under the bare property"
        );
        assert!(
            parse(r#"upnp:artist[@role="Composer"] contains "beethoven""#)
                .expect("it parses")
                .matches(&object),
            "and the role still reaches him"
        );
    }

    #[test]
    fn a_negation_holds_for_an_object_carrying_no_such_value_at_all() {
        let criteria = parse(r#"upnp:genre doesNotContain "metal""#).expect("it parses");
        assert!(criteria.matches(&track()));
    }

    #[test]
    fn nothing_this_server_serves_is_a_reference() {
        let track = track();
        assert!(
            parse("@refID exists false")
                .expect("it parses")
                .matches(&track)
        );
        assert!(
            !parse("@refID exists true")
                .expect("it parses")
                .matches(&track)
        );
    }

    #[test]
    fn the_star_matches_every_object() {
        assert!(parse("*").expect("it parses").matches(&track()));
    }

    #[test]
    fn the_capability_string_is_the_evaluated_set() {
        let published = capabilities();
        let declared: Vec<&str> = published.split(',').collect();
        assert_eq!(declared.len(), EVALUATED.len());
        for (declared, property) in declared.iter().zip(EVALUATED) {
            assert_eq!(*declared, property.to_string());
        }
    }
}
