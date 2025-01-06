// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! References and reference related utilities

use std::error::Error;
use std::path::Path;

use crate::base::Base;

/// Some entity that stores [Reference]s
pub trait Store<'r>: for<'a> Base<Reference<'a>: Reference> {}

impl Store<'_> for git2::Repository {}

/// A git reference
pub trait Reference {
    /// Type for reference names
    type Name: ?Sized;

    /// Type used for representing Object IDs
    type Oid: std::str::FromStr;

    /// [Error] type used for communicating name and path retrieval errors
    type Error: Error;

    /// Retrieve the name of the reference
    fn name(&self) -> Result<&Self::Name, Self::Error>;

    /// Retrieve the [Path] representation of this reference
    fn as_path(&self) -> Result<&Path, Self::Error>;

    /// Extract the defining parts of this reference regarding the issue
    fn parts(&self) -> Option<Parts<'_, Self::Oid>> {
        let mut path = self.as_path().ok()?;

        let kind = if path.ends_with(HEAD_COMPONENT) {
            Kind::Head
        } else {
            let id = path.file_name()?.to_str()?.parse().ok()?;
            path = path.parent()?;
            path.ends_with(LEAF_COMPONENT).then_some(())?;
            Kind::Leaf(id)
        };

        path = path.parent()?;

        let issue = path.file_name()?.to_str()?.parse().ok()?;
        path.parent().map(|prefix| Parts {
            prefix,
            issue,
            kind,
        })
    }

    /// Retrieve the target of this reference
    ///
    /// This fn will return the target if this reference is direct. For indirect
    /// references, this fn will return [None].
    fn target(&self) -> Option<Self::Oid>;
}

impl Reference for git2::Reference<'_> {
    type Name = str;
    type Oid = git2::Oid;
    type Error = std::str::Utf8Error;

    fn name(&self) -> Result<&Self::Name, Self::Error> {
        std::str::from_utf8(self.name_bytes())
    }

    fn as_path(&self) -> Result<&Path, Self::Error> {
        Reference::name(self).map(Path::new)
    }

    fn target(&self) -> Option<Self::Oid> {
        self.target()
    }
}

/// Parts of a [Reference] associated to an issue
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Parts<'p, O> {
    /// Path or namespace under which the issue resides
    pub prefix: &'p Path,
    /// Id of the associated issue
    pub issue: O,
    /// Kind of [Reference]
    pub kind: Kind<O>,
}

/// Kind of reference
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind<O> {
    /// The reference is a head reference for an issue
    Head,
    /// The reference is a leaf reference for an issue
    Leaf(O),
}

/// Identifier/file name for the head reference of an issue
pub(crate) const HEAD_COMPONENT: &str = "head";

/// Identifier for leaf namespace in an issue
pub(crate) const LEAF_COMPONENT: &str = "leaves";

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::base::tests::TestOid;
    use crate::error::tests::TestError;

    #[derive(Clone, Debug)]
    pub struct TestRef {
        name: std::path::PathBuf,
        target: Option<TestOid>,
    }

    impl TestRef {
        pub fn with_target(self, target: TestOid) -> Self {
            Self {
                target: Some(target),
                ..self
            }
        }
    }

    impl From<&str> for TestRef {
        fn from(path: &str) -> Self {
            Self {
                name: path.into(),
                target: None,
            }
        }
    }

    impl Reference for TestRef {
        type Name = str;
        type Oid = TestOid;
        type Error = TestError;

        fn name(&self) -> Result<&Self::Name, Self::Error> {
            self.name.to_str().ok_or(TestError)
        }

        fn as_path(&self) -> Result<&Path, Self::Error> {
            Ok(self.name.as_ref())
        }

        fn target(&self) -> Option<Self::Oid> {
            self.target
        }
    }

    #[test]
    fn ref_parts_headref() {
        let reference = TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/head");
        let parts = reference.parts().expect("Could not extract parts");
        assert_eq!(parts.prefix, Path::new("refs/dit"));
        assert_eq!(parts.issue, "65b56706fdc3501749d008750c61a1f24b888f72");
        assert_eq!(parts.kind, Kind::Head);
    }

    #[test]
    fn ref_parts_leaf() {
        let reference = TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/leaves/f6bd121bdc2ba5906e412da19191a2eaf2025755");
        let parts = reference.parts().expect("Could not extract parts");
        assert_eq!(parts.prefix, Path::new("refs/dit"));
        assert_eq!(parts.issue, "65b56706fdc3501749d008750c61a1f24b888f72");
        assert_eq!(
            parts.kind,
            Kind::Leaf(
                "f6bd121bdc2ba5906e412da19191a2eaf2025755"
                    .parse()
                    .expect("Could not parse leaf OId")
            )
        );
    }

    #[test]
    fn ref_parts_invalid_head_1() {
        assert_eq!(
            TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/head/foo").parts(),
            None,
        );
    }

    #[test]
    fn ref_parts_invalid_head_2() {
        assert_eq!(TestRef::from("refs/dit/foo/head").parts(), None);
    }

    #[test]
    fn ref_parts_invalid_leaf_1() {
        assert_eq!(TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/foo/f6bd121bdc2ba5906e412da19191a2eaf2025755").parts(), None);
    }

    #[test]
    fn ref_parts_invalid_leaf_2() {
        assert_eq!(
            TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/leaves/foo").parts(),
            None,
        );
    }

    #[test]
    fn ref_parts_invalid_leaf_3() {
        assert_eq!(
            TestRef::from("refs/dit/foo/leaves/f6bd121bdc2ba5906e412da19191a2eaf2025755").parts(),
            None,
        );
    }
}
