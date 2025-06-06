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
use crate::error::{self, InnerError, ResultExt};
use crate::remote;

/// Some entity that stores [Reference]s
pub trait Store<'r>: Base {
    /// Type used for representing references
    type Reference: Reference<
        Oid = Self::Oid,
        Name: ToOwned<Owned = <<Self as Base>::InnerError as InnerError>::RefName>,
        Error: Into<error::Inner<Self::InnerError>>,
    >;

    /// Type for a basic [Iterator] of [Reference]s
    type References: IntoIterator<Item = Result<Self::Reference, Self::InnerError>>;

    /// Container for remote references' names
    type RemoteNames: remote::Names;

    /// Retrieve a specific reference
    fn get_reference(
        &'r self,
        path: &Path,
    ) -> error::Result<Option<Self::Reference>, Self::InnerError>;

    /// Retrieve a subset of all [Reference]s in this store
    fn references(&'r self, prefix: &Path) -> error::Result<Self::References, Self::InnerError>;

    /// Update or create a new [Reference]
    fn set_reference(
        &'r self,
        name: &Path,
        target: Self::Oid,
        overwrite: bool,
        reflog_msg: &str,
    ) -> error::Result<Self::Reference, Self::InnerError>;

    /// Retrieve all git remote references' names
    fn remote_names(&self) -> error::Result<Self::RemoteNames, Self::InnerError>;

    /// Retrieve all git remote references' ref paths
    fn remote_ref_paths(&self) -> error::Result<Vec<String>, Self::InnerError> {
        use remote::Names;

        self.remote_names()?
            .ref_paths()
            .map(|n| n.wrap_with_kind(error::Kind::ReferenceNameError))
            .collect()
    }
}

#[cfg(feature = "git2")]
impl<'r> Store<'r> for git2::Repository {
    type Reference = git2::Reference<'r>;
    type References = git2::References<'r>;
    type RemoteNames = git2::string_array::StringArray;

    fn get_reference(
        &'r self,
        path: &Path,
    ) -> error::Result<Option<Self::Reference>, Self::InnerError> {
        let name = path.to_str().ok_or(error::Kind::CannotGetReference)?;
        match self.find_reference(name).map(Some) {
            Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(None),
            err => err.wrap_with_kind(error::Kind::CannotGetReference),
        }
    }

    fn references(&'r self, prefix: &Path) -> error::Result<Self::References, Self::InnerError> {
        let glob = format!("{}/**", prefix.display());
        self.references_glob(glob.as_ref())
            .wrap_with_kind(error::Kind::CannotGetReferences(glob))
    }

    fn set_reference(
        &'r self,
        name: &Path,
        target: Self::Oid,
        overwrite: bool,
        reflog_msg: &str,
    ) -> error::Result<Self::Reference, Self::InnerError> {
        let path = name.to_str().ok_or(error::Kind::ReferenceNameError)?;
        self.reference(path, target, overwrite, reflog_msg)
            .wrap_with(|| error::Kind::CannotSetReference(path.to_owned()))
    }

    fn remote_names(&self) -> error::Result<Self::RemoteNames, Self::InnerError> {
        self.remotes().wrap_with_kind(error::Kind::CannotGetRemotes)
    }
}

/// Extension trait for [Iterator]s over [Reference]s
pub trait References {
    /// [Reference] yielded by this [Iterator]
    type Reference: Reference;

    /// Errors yielded by this [Iterator]
    type Error;

    /// Yield only head references
    fn heads(self) -> impl Iterator<Item = Result<Self::Reference, Self::Error>>;

    /// Yield only leaf references
    fn leaves(self) -> impl Iterator<Item = Result<Self::Reference, Self::Error>>;
}

impl<T, R, E> References for T
where
    T: IntoIterator<Item = Result<R, E>>,
    R: Reference,
{
    type Reference = R;
    type Error = E;

    fn heads(self) -> impl Iterator<Item = Result<Self::Reference, Self::Error>> {
        self.into_iter()
            .filter(|r| r.as_ref().map(Reference::is_head).unwrap_or(true))
    }

    fn leaves(self) -> impl Iterator<Item = Result<Self::Reference, Self::Error>> {
        self.into_iter()
            .filter(|r| r.as_ref().map(Reference::is_leaf).unwrap_or(true))
    }
}

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

    /// Check whether this is an issue head reference
    fn is_head(&self) -> bool {
        self.parts()
            .map(|p| matches!(p.kind, Kind::Head))
            .unwrap_or(false)
    }

    /// Check whether this is an issue leaf reference
    fn is_leaf(&self) -> bool {
        self.parts()
            .map(|p| matches!(p.kind, Kind::Leaf(_)))
            .unwrap_or(false)
    }

    /// Retrieve the target of this reference
    ///
    /// This fn will return the target if this reference is direct. For indirect
    /// references, this fn will return [None].
    fn target(&self) -> Option<Self::Oid>;
}

#[cfg(feature = "git2")]
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

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use crate::base::tests::TestOid;
    use crate::error::tests::TestError;

    impl<'r, T> Store<'r> for (TestStore, T)
    where
        T: Base<Oid = <TestStore as Base>::Oid, InnerError = <TestStore as Base>::InnerError>,
    {
        type Reference = <TestStore as Store<'r>>::Reference;
        type References = <TestStore as Store<'r>>::References;
        type RemoteNames = <TestStore as Store<'r>>::RemoteNames;

        fn get_reference(
            &'r self,
            path: &Path,
        ) -> error::Result<Option<Self::Reference>, Self::InnerError> {
            self.0.get_reference(path)
        }

        fn references(
            &'r self,
            prefix: &Path,
        ) -> error::Result<Self::References, Self::InnerError> {
            self.0.references(prefix)
        }

        fn set_reference(
            &'r self,
            name: &Path,
            target: Self::Oid,
            overwrite: bool,
            reflog_msg: &str,
        ) -> error::Result<Self::Reference, Self::InnerError> {
            self.0.set_reference(name, target, overwrite, reflog_msg)
        }

        fn remote_names(&self) -> error::Result<Self::RemoteNames, Self::InnerError> {
            self.0.remote_names()
        }
    }

    #[derive(Default)]
    pub struct TestStore {
        refs: std::sync::Mutex<BTreeSet<TestRef>>,
        remotes: Vec<String>,
    }

    impl<'r> Store<'r> for TestStore {
        type Reference = TestRef;
        type References = Vec<Result<TestRef, TestError>>;
        type RemoteNames = Vec<String>;

        fn get_reference(
            &'r self,
            path: &Path,
        ) -> error::Result<Option<Self::Reference>, Self::InnerError> {
            Ok(self
                .refs
                .lock()
                .expect("Could not access refs")
                .get(path)
                .cloned())
        }

        fn references(
            &'r self,
            prefix: &Path,
        ) -> error::Result<Self::References, Self::InnerError> {
            let res = self
                .refs
                .lock()
                .expect("Could not access refs")
                .iter()
                .filter(|r| r.name.starts_with(prefix))
                .cloned()
                .map(Ok)
                .collect();
            Ok(res)
        }

        fn set_reference(
            &'r self,
            name: &Path,
            target: Self::Oid,
            overwrite: bool,
            _reflog_msg: &str,
        ) -> error::Result<Self::Reference, Self::InnerError> {
            let new = TestRef::from(name.to_owned()).with_target(target);
            let mut refs = self.refs.lock().expect("Could not access refs");
            if overwrite {
                refs.replace(new.clone());
            } else {
                refs.insert(new.clone());
            }

            Ok(new)
        }

        fn remote_names(&self) -> error::Result<Self::RemoteNames, Self::InnerError> {
            Ok(self.remotes.clone())
        }
    }

    impl Base for TestStore {
        type Oid = TestOid;
        type InnerError = TestError;
    }

    #[derive(Clone, Debug)]
    pub struct TestRef {
        name: PathBuf,
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

    impl From<PathBuf> for TestRef {
        fn from(name: PathBuf) -> Self {
            Self { name, target: None }
        }
    }

    impl From<&str> for TestRef {
        fn from(path: &str) -> Self {
            PathBuf::from(path).into()
        }
    }

    impl std::borrow::Borrow<Path> for TestRef {
        fn borrow(&self) -> &Path {
            &self.name
        }
    }

    impl Eq for TestRef {}

    impl PartialEq for TestRef {
        fn eq(&self, other: &Self) -> bool {
            PartialEq::eq(&self.name, &other.name)
        }
    }

    impl Ord for TestRef {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            Ord::cmp(&self.name, &other.name)
        }
    }

    impl PartialOrd for TestRef {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            PartialOrd::partial_cmp(&self.name, &other.name)
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
