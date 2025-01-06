// git-dit - the distributed issue tracker for git
// Copyright (C) 2025 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//! References and reference related utilities

use std::borrow::Cow;
use std::path::Path;

/// A git reference
pub trait Reference<'r> {
    /// Type for reference names
    type Name;

    /// Type for holding [Path] represenations of references
    type Path: AsRef<Path>;

    /// Type used for representing Object IDs
    type Oid;

    /// Retrieve the name of the reference
    fn name(&'r self) -> Self::Name;

    /// Retrieve the [Path] representation of this reference
    fn as_path(&'r self) -> Self::Path;

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
}
