use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::EndpointCapability;
use crate::capability::AddressPolicy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolverFailureKind {
    LookupFailed,
    AddressPolicyDenied,
}

#[derive(Debug)]
struct ResolverFailure(ResolverFailureKind);

impl fmt::Display for ResolverFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            ResolverFailureKind::LookupFailed => "DNS lookup failed",
            ResolverFailureKind::AddressPolicyDenied => "DNS address policy denied destination",
        })
    }
}

impl Error for ResolverFailure {}

#[derive(Clone, Debug)]
pub(crate) struct GuardedResolver {
    host_policies: BTreeMap<String, AddressPolicy>,
    timeout: Duration,
}

impl GuardedResolver {
    pub(crate) fn new(endpoints: &[EndpointCapability], timeout: Duration) -> Self {
        let host_policies = endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.host().to_ascii_lowercase(),
                    endpoint.address_policy(),
                )
            })
            .collect();
        Self {
            host_policies,
            timeout,
        }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_ascii_lowercase();
        let policy = self.host_policies.get(&host).copied();
        let timeout = self.timeout;

        Box::pin(async move {
            let Some(policy) = policy else {
                return Err(
                    Box::new(ResolverFailure(ResolverFailureKind::AddressPolicyDenied))
                        as Box<dyn Error + Send + Sync>,
                );
            };

            let lookup = tokio::time::timeout(timeout, tokio::net::lookup_host((host.as_str(), 0)))
                .await
                .map_err(|_| {
                    Box::new(ResolverFailure(ResolverFailureKind::LookupFailed))
                        as Box<dyn Error + Send + Sync>
                })?
                .map_err(|_| {
                    Box::new(ResolverFailure(ResolverFailureKind::LookupFailed))
                        as Box<dyn Error + Send + Sync>
                })?;

            let mut addresses: Vec<SocketAddr> = lookup
                .filter(|address| policy.permits(address.ip()))
                .collect();
            addresses.sort_unstable();
            addresses.dedup();
            if addresses.is_empty() {
                return Err(
                    Box::new(ResolverFailure(ResolverFailureKind::AddressPolicyDenied))
                        as Box<dyn Error + Send + Sync>,
                );
            }

            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

pub(crate) fn resolver_failure_kind(error: &(dyn Error + 'static)) -> Option<ResolverFailureKind> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(failure) = error.downcast_ref::<ResolverFailure>() {
            return Some(failure.0);
        }
        current = error.source();
    }
    None
}
