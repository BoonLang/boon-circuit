//! Opaque, construction-only checked-image publication.
//!
//! This module is intentionally outside the serialized/public classifier
//! inventory. The kernel linker builds the value, the compiler appends its
//! project metadata rows, and the typechecker consumes it exactly once.

use crate::{
    CheckedImageRowDomainV2, CheckedShardProjectionKeyV2, ProgramRole, SourceBundleDigestV1,
};
use std::collections::{BTreeMap, HashMap};

/// Dense, move-only checked-image publication assembled by the kernel linker.
///
/// This is deliberately not a serialized checked artifact. It carries only
/// projection topology, row cardinalities, and exact entity routes; the
/// definition/currentness authority remains in the independently sealed
/// checked-image authority.
#[derive(Debug, Eq, PartialEq)]
pub struct CheckedImageKernelPublicationV1 {
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    projection_ids: BTreeMap<CheckedShardProjectionKeyV2, CheckedImageKernelProjectionIdV1>,
    projections: Vec<CheckedImageKernelProjectionV1>,
    routes: HashMap<(CheckedImageRowDomainV2, u32), CheckedImageKernelProjectionIdV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CheckedImageKernelProjectionIdV1(u32);

impl CheckedImageKernelProjectionIdV1 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CheckedImageKernelProjectionV1 {
    key: CheckedShardProjectionKeyV2,
    row_count: u32,
    dependency_row_count: u32,
    relocations: Vec<CheckedImageKernelProjectionIdV1>,
}

impl CheckedImageKernelPublicationV1 {
    #[doc(hidden)]
    pub fn __kernel_new(source_bundle_digest_v1: SourceBundleDigestV1, role: ProgramRole) -> Self {
        Self {
            source_bundle_digest_v1,
            role,
            projection_ids: BTreeMap::new(),
            projections: Vec::new(),
            routes: HashMap::new(),
        }
    }

    #[doc(hidden)]
    pub fn __kernel_intern_projection(
        &mut self,
        key: CheckedShardProjectionKeyV2,
    ) -> Result<CheckedImageKernelProjectionIdV1, String> {
        if let Some(id) = self.projection_ids.get(&key).copied() {
            return Ok(id);
        }
        let id = CheckedImageKernelProjectionIdV1(
            u32::try_from(self.projections.len())
                .map_err(|_| "kernel checked-image projection count exceeds u32".to_owned())?,
        );
        self.projection_ids.insert(key.clone(), id);
        self.projections.push(CheckedImageKernelProjectionV1 {
            key,
            row_count: 0,
            dependency_row_count: 0,
            relocations: Vec::new(),
        });
        Ok(id)
    }

    #[doc(hidden)]
    pub fn __kernel_publish_rows(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        row_count: u32,
    ) -> Result<(), String> {
        let row = self
            .projections
            .get_mut(projection.as_usize())
            .ok_or_else(|| {
                format!(
                    "kernel checked-image publication references missing projection {}",
                    projection.0
                )
            })?;
        row.row_count = row
            .row_count
            .checked_add(row_count)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_publish_dependency_row(
        &mut self,
        projection: CheckedImageKernelProjectionIdV1,
        relocations: impl IntoIterator<Item = CheckedImageKernelProjectionIdV1>,
    ) -> Result<(), String> {
        let mut relocations = relocations.into_iter().collect::<Vec<_>>();
        relocations.sort_unstable();
        relocations.dedup();
        relocations.retain(|target| *target != projection);
        let projection_count = self.projections.len();
        if relocations
            .iter()
            .any(|target| target.as_usize() >= projection_count)
        {
            return Err(
                "kernel checked-image dependency references a missing projection".to_owned(),
            );
        }
        let row = self
            .projections
            .get_mut(projection.as_usize())
            .ok_or_else(|| {
                format!(
                    "kernel checked-image publication references missing projection {}",
                    projection.0
                )
            })?;
        row.row_count = row
            .row_count
            .checked_add(1)
            .ok_or_else(|| "kernel checked-image row count exceeds u32".to_owned())?;
        if !relocations.is_empty() {
            row.dependency_row_count = row
                .dependency_row_count
                .checked_add(1)
                .ok_or_else(|| "kernel checked-image dependency count exceeds u32".to_owned())?;
            row.relocations.extend(relocations);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_route(
        &mut self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Result<(), String> {
        if projection.as_usize() >= self.projections.len() {
            return Err("kernel checked-image route references a missing projection".to_owned());
        }
        let dense_index = u32::try_from(dense_index)
            .map_err(|_| "kernel checked-image route exceeds u32".to_owned())?;
        if self
            .routes
            .insert((domain, dense_index), projection)
            .is_some()
        {
            return Err(format!(
                "kernel checked-image {domain:?} route {dense_index} is published twice"
            ));
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn __kernel_projection_for_route(
        &self,
        domain: CheckedImageRowDomainV2,
        dense_index: usize,
    ) -> Option<CheckedImageKernelProjectionIdV1> {
        u32::try_from(dense_index)
            .ok()
            .and_then(|dense_index| self.routes.get(&(domain, dense_index)).copied())
    }

    #[doc(hidden)]
    pub fn __kernel_projection_key(
        &self,
        projection: CheckedImageKernelProjectionIdV1,
    ) -> Option<&CheckedShardProjectionKeyV2> {
        self.projections
            .get(projection.as_usize())
            .map(|projection| &projection.key)
    }

    #[doc(hidden)]
    pub fn __typechecker_into_parts(
        self,
    ) -> (
        SourceBundleDigestV1,
        ProgramRole,
        Vec<(
            CheckedShardProjectionKeyV2,
            u32,
            u32,
            Vec<CheckedImageKernelProjectionIdV1>,
        )>,
        HashMap<(CheckedImageRowDomainV2, u32), CheckedImageKernelProjectionIdV1>,
    ) {
        (
            self.source_bundle_digest_v1,
            self.role,
            self.projections
                .into_iter()
                .map(|projection| {
                    (
                        projection.key,
                        projection.row_count,
                        projection.dependency_row_count,
                        projection.relocations,
                    )
                })
                .collect(),
            self.routes,
        )
    }
}
