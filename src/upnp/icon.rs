//! The picture a control point shows beside this server.

use std::path::Path;

use crate::index::artwork;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendering {
    pub name: &'static str,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: &'static [u8],
}

/// Largest first: a control point takes the first entry.
pub const RENDERINGS: [Rendering; 4] = [
    Rendering {
        name: "icon-120.png",
        mime: "image/png",
        width: 120,
        height: 120,
        bytes: include_bytes!("../../assets/icon-120.png"),
    },
    Rendering {
        name: "icon-120.jpg",
        mime: "image/jpeg",
        width: 120,
        height: 120,
        bytes: include_bytes!("../../assets/icon-120.jpg"),
    },
    Rendering {
        name: "icon-48.png",
        mime: "image/png",
        width: 48,
        height: 48,
        bytes: include_bytes!("../../assets/icon-48.png"),
    },
    Rendering {
        name: "icon-48.jpg",
        mime: "image/jpeg",
        width: 48,
        height: 48,
        bytes: include_bytes!("../../assets/icon-48.jpg"),
    },
];

const DEPTH: u32 = 24;

pub const PREFERRED: &str = "icon-120.png";

#[derive(Clone, Debug, Default)]
pub enum Icon {
    #[default]
    Shipped,
    Own(Own),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Own {
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl Icon {
    pub fn read(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "no icon there; using the shipped one");
                return Self::Shipped;
            }
        };
        let Some(mime) = artwork::image_mime(&bytes) else {
            tracing::warn!(
                path = %path.display(),
                "an icon must be a PNG or a JPEG, whatever its name says; using the shipped one"
            );
            return Self::Shipped;
        };
        let Some((width, height)) = artwork::dimensions(&bytes) else {
            tracing::warn!(
                path = %path.display(),
                "the size of that icon could not be read; using the shipped one"
            );
            return Self::Shipped;
        };
        Self::Own(Own {
            mime,
            width,
            height,
            bytes,
        })
    }

    pub fn served(&self, name: &str) -> Option<(&'static str, &[u8])> {
        match self {
            Self::Own(own) if name == PREFERRED => Some((own.mime, &own.bytes)),
            Self::Own(_) => None,
            Self::Shipped => RENDERINGS
                .iter()
                .find(|rendering| rendering.name == name)
                .map(|rendering| (rendering.mime, rendering.bytes)),
        }
    }

    pub fn list(&self) -> String {
        let entry = |mime: &str, width: u32, height: u32, name: &str| {
            format!(
                "      <icon><mimetype>{mime}</mimetype><width>{width}</width>\
                 <height>{height}</height><depth>{DEPTH}</depth><url>/icon/{name}</url></icon>\n"
            )
        };
        let entries = match self {
            Self::Own(own) => entry(own.mime, own.width, own.height, PREFERRED),
            Self::Shipped => RENDERINGS
                .iter()
                .map(|it| entry(it.mime, it.width, it.height, it.name))
                .collect(),
        };
        format!("    <iconList>\n{entries}    </iconList>\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_rendering_is_the_format_and_the_size_it_claims() {
        for rendering in &RENDERINGS {
            assert_eq!(
                artwork::image_mime(rendering.bytes),
                Some(rendering.mime),
                "{} says one thing and its bytes another",
                rendering.name
            );
            assert_eq!(
                artwork::dimensions(rendering.bytes),
                Some((rendering.width, rendering.height)),
                "{} is not the size the description will claim",
                rendering.name
            );
        }
    }

    #[test]
    fn the_list_names_a_url_this_server_answers() {
        let icon = Icon::Shipped;
        let list = icon.list();
        for rendering in &RENDERINGS {
            assert!(list.contains(&format!("<url>/icon/{}</url>", rendering.name)));
            assert!(icon.served(rendering.name).is_some());
        }
        assert!(icon.served("icon-999.png").is_none());
    }

    #[test]
    fn an_icon_that_is_not_an_image_is_refused_rather_than_published() {
        let path = std::env::temp_dir().join("kantele-not-an-icon.png");
        std::fs::write(&path, b"this is not a png").expect("the fixture writes");
        assert!(matches!(Icon::read(&path), Icon::Shipped));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_owners_icon_replaces_the_shipped_list_with_its_own_measured_size() {
        let path = std::env::temp_dir().join("kantele-own-icon.png");
        let shipped = &RENDERINGS[2];
        std::fs::write(&path, shipped.bytes).expect("the fixture writes");
        let icon = Icon::read(&path);
        assert!(icon.list().contains("<width>48</width>"));
        assert_eq!(icon.list().matches("<icon>").count(), 1);
        assert!(icon.served(PREFERRED).is_some());
        let _ = std::fs::remove_file(&path);
    }
}
