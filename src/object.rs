//! What a catalogue object is called and what kind it is, named by both the index and the wire.

/// The DIDL-Lite classes this server publishes.
pub const ALBUM: &str = "object.container.album.musicAlbum";
pub const ARTIST: &str = "object.container.person.musicArtist";
pub const PLAYLIST: &str = "object.container.playlistContainer";
pub const MUSIC_TRACK: &str = "object.item.audioItem.musicTrack";

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObjectIdError {
    #[error("object id must not be empty")]
    Empty,
    #[error("object id contains {character:?} at byte {offset}, outside A-Za-z0-9-._~")]
    IllegalCharacter { character: char, offset: usize },
}

impl ObjectId {
    /// The specification fixes the root container as `0`.
    pub const ROOT: &'static str = "0";

    pub fn new(value: impl Into<String>) -> Result<Self, ObjectIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ObjectIdError::Empty);
        }
        if let Some((offset, character)) = value.char_indices().find(|(_, c)| !Self::is_legal(*c)) {
            return Err(ObjectIdError::IllegalCharacter { character, offset });
        }
        Ok(Self(value))
    }

    pub fn root() -> Self {
        Self(Self::ROOT.to_owned())
    }

    pub const fn is_legal(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_zero() {
        assert_eq!(ObjectId::root().as_str(), "0");
    }

    #[test]
    fn the_unreserved_alphabet_is_accepted() {
        for value in ["0", "a1", "album-4f2a", "track_9.disc~2", "AZaz09-._~"] {
            assert!(ObjectId::new(value).is_ok(), "{value} should be accepted");
        }
    }

    #[test]
    fn characters_the_incumbents_use_are_refused() {
        for value in [
            "0$albums",
            "0$albums$*a0$*i20885",
            "0$=Composer",
            "audio/Artist/Album",
        ] {
            assert!(
                matches!(
                    ObjectId::new(value),
                    Err(ObjectIdError::IllegalCharacter { .. })
                ),
                "{value} should be refused"
            );
        }
    }

    #[test]
    fn anything_needing_escaping_is_refused() {
        for value in ["a b", "a&b", "a<b", "a%20b", "a+b", "café"] {
            assert!(ObjectId::new(value).is_err(), "{value} should be refused");
        }
    }

    #[test]
    fn the_error_names_the_offending_character_and_where_it_is() {
        assert_eq!(
            ObjectId::new("album$1"),
            Err(ObjectIdError::IllegalCharacter {
                character: '$',
                offset: 5,
            })
        );
    }

    #[test]
    fn empty_is_refused() {
        assert_eq!(ObjectId::new(""), Err(ObjectIdError::Empty));
    }
}
