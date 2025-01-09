// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! References and reference related utilities

use std::borrow::Cow;
use std::path::Path;

use poppable_path::Poppable;

/// A git reference
pub trait Reference<'r> {
    /// Type for reference names
    type Name;

    /// Type for holding [Path] represenations of references
    type Path: Poppable + AsRef<Path>;

    /// Type used for representing Object IDs
    type Oid: std::str::FromStr;

    /// Retrieve the name of the reference
    fn name(&'r self) -> Self::Name;

    /// Retrieve the [Path] representation of this reference
    fn as_path(&'r self) -> Self::Path;

    /// Extract the defining parts of this reference regarding the issue
    fn parts(&'r self) -> Option<Parts<Self::Path, Self::Oid>> {
        let mut path = self.as_path();

        let kind = if path.as_ref().ends_with(HEAD_COMPONENT) {
            Kind::Head
        } else {
            let id = path.as_ref().file_name()?.to_str()?.parse().ok()?;
            path.pop().then_some(())?;
            path.as_ref().ends_with(LEAF_COMPONENT).then_some(())?;
            Kind::Leaf(id)
        };

        path.pop().then_some(())?;

        let issue = path.as_ref().file_name()?.to_str()?.parse().ok()?;
        path.pop().then_some(Parts {
            prefix: path,
            issue,
            kind,
        })
    }

    /// Retrieve the target of this reference
    ///
    /// This fn will return the target if this reference is direct. For indirect
    /// references, this fn will return [None].
    fn target(&'r self) -> Option<Self::Oid>;
}

impl<'r> Reference<'r> for git2::Reference<'_> {
    type Name = Cow<'r, str>;
    type Path = Cow<'r, Path>;
    type Oid = git2::Oid;

    fn name(&'r self) -> Self::Name {
        String::from_utf8_lossy(self.name_bytes())
    }

    fn as_path(&'r self) -> Self::Path {
        match Reference::name(self) {
            Cow::Borrowed(p) => Cow::Borrowed(Path::new(p)),
            Cow::Owned(p) => Cow::Owned(p.into()),
        }
    }

    fn target(&'r self) -> Option<Self::Oid> {
        self.target()
    }
}

/// Parts of a [Reference] associated to an issue
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Parts<P, O> {
    /// Path or namespace under which the issue resides
    pub prefix: P,
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

    impl<'r> Reference<'r> for TestRef {
        type Name = Cow<'r, str>;
        type Path = std::path::PathBuf;
        type Oid = TestOid;

        fn name(&'r self) -> Self::Name {
            self.name.to_string_lossy()
        }

        fn as_path(&'r self) -> Self::Path {
            self.name.clone()
        }

        fn target(&'r self) -> Option<Self::Oid> {
            self.target
        }
    }

    #[test]
    fn ref_parts_headref() {
        let parts = TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/head")
            .parts()
            .expect("Could not extract parts");
        assert_eq!(parts.prefix, Path::new("refs/dit"));
        assert_eq!(parts.issue, "65b56706fdc3501749d008750c61a1f24b888f72");
        assert_eq!(parts.kind, Kind::Head);
    }

    #[test]
    fn ref_parts_leaf() {
        let parts = TestRef::from("refs/dit/65b56706fdc3501749d008750c61a1f24b888f72/leaves/f6bd121bdc2ba5906e412da19191a2eaf2025755")
            .parts()
            .expect("Could not extract parts");
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
