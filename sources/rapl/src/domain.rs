use std::fmt::Display;

use joule_profiler_core::metric::MetricValue;
use joule_profiler_core::unit::MetricUnit;

use crate::error::{RaplError, Result};

#[cfg(feature = "backend-perf")]
use {
    crate::{counter::build_counter, perf::PMU},
    joule_profiler_core::util::sys::{read_cpu_list, socket_topology},
    perf_event::events::Dynamic,
    std::{collections::HashSet, io::ErrorKind, path::Path},
};

/// The CPUs the `power` PMU counts on, one per socket.
#[cfg(feature = "backend-perf")]
const PMU_CPUMASK: &str = "/sys/bus/event_source/devices/power/cpumask";

/// The unit the energy is reported in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnergyUnit {
    Joule,
    Millijoule,
    #[default]
    Microjoule,
}

impl EnergyUnit {
    pub(crate) fn metric_unit(self) -> MetricUnit {
        match self {
            Self::Joule => MetricUnit::JOULE,
            Self::Millijoule => MetricUnit::MILLIJOULE,
            Self::Microjoule => MetricUnit::MICROJOULE,
        }
    }

    pub fn per_joule(self) -> f64 {
        match self {
            Self::Joule => 1.0,
            Self::Millijoule => 1e3,
            Self::Microjoule => 1e6,
        }
    }

    /// `energy`, in this unit, as whole millijoules or microjoules but fractional joules.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an energy is positive, and a phase never accumulates 2^64 microjoules"
    )]
    pub(crate) fn value(self, energy: f64) -> MetricValue {
        match self {
            Self::Joule => MetricValue::F64(energy),
            Self::Millijoule | Self::Microjoule => MetricValue::U64(energy.round() as u64),
        }
    }
}

#[cfg(feature = "backend-perf")]
pub(crate) const DOMAINS: [RaplDomainType; 5] = [
    RaplDomainType::Package,
    RaplDomainType::Core,
    RaplDomainType::Uncore,
    RaplDomainType::Dram,
    RaplDomainType::Psys,
];

#[cfg(feature = "backend-perf")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct RaplDomain {
    pub domain: RaplDomainType,
    pub socket: u32,
    pub cpu: u32,
    pub scale: f64,
}

#[cfg(feature = "backend-perf")]
impl RaplDomain {
    pub fn name(&self) -> String {
        self.domain.to_string_socket(self.socket)
    }
}

/// A RAPL (Running Average Power Limit) energy domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RaplDomainType {
    /// The whole socket (PKG).
    Package,

    /// The cores (PP0).
    Core,

    /// The integrated GPU (PP1).
    Uncore,

    /// The memory attached to the socket.
    Dram,

    /// The whole platform, not tied to a socket.
    Psys,
}

impl RaplDomainType {
    /// `PACKAGE-0`, or `PSYS` which has no socket.
    pub fn to_string_socket(self, socket: u32) -> String {
        match self {
            RaplDomainType::Psys => RaplDomainType::Psys.to_string(),
            domain_type => format!("{domain_type}-{socket}"),
        }
    }

    /// The event name in the `power` PMU.
    pub fn to_perf_event(self) -> &'static str {
        match self {
            RaplDomainType::Package => "energy-pkg",
            RaplDomainType::Core => "energy-cores",
            RaplDomainType::Uncore => "energy-gpu",
            RaplDomainType::Dram => "energy-ram",
            RaplDomainType::Psys => "energy-psys",
        }
    }
}

impl Display for RaplDomainType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let domain_type = match self {
            RaplDomainType::Package => "PACKAGE",
            RaplDomainType::Core => "CORE",
            RaplDomainType::Uncore => "UNCORE",
            RaplDomainType::Dram => "DRAM",
            RaplDomainType::Psys => "PSYS",
        };
        f.write_str(domain_type)
    }
}

impl TryFrom<&str> for RaplDomainType {
    type Error = RaplError;

    fn try_from(value: &str) -> Result<Self> {
        let eq = |s| value.eq_ignore_ascii_case(s);

        let domain_type = if eq("core") || eq("pp0") || eq("energy-cores") {
            RaplDomainType::Core
        } else if eq("uncore") || eq("pp1") || eq("energy-uncore") || eq("energy-gpu") {
            RaplDomainType::Uncore
        } else if eq("dram") || eq("ram") || eq("energy-ram") {
            RaplDomainType::Dram
        } else if (value.len() >= 7 && value[..7].eq_ignore_ascii_case("package"))
            || eq("energy-pkg")
        {
            RaplDomainType::Package
        } else if eq("psys") || eq("platform") || eq("energy-psys") {
            RaplDomainType::Psys
        } else {
            return Err(RaplError::UnknownDomain(value.to_string()));
        };

        Ok(domain_type)
    }
}

#[cfg(feature = "backend-perf")]
pub(crate) fn discover_domains(
    sockets_spec: Option<&HashSet<u32>>,
    domains_filter: Option<&HashSet<RaplDomainType>>,
    unit: EnergyUnit,
) -> Result<Vec<RaplDomain>> {
    let mut available = Vec::new();

    let domains = match domains_filter {
        Some(d) => d.iter().copied().collect::<Vec<_>>(),
        None => DOMAINS.to_vec(),
    };

    for domain in domains {
        if let Some(scale) = get_domain_scale(domain)? {
            available.push((domain, scale * unit.per_joule()));
        }
    }

    let topology = socket_topology().map_err(RaplError::Topology)?;

    if let Some(wanted) = sockets_spec
        && let Some(unknown) = wanted
            .iter()
            .find(|id| !topology.iter().any(|socket| socket.id == **id))
    {
        return Err(RaplError::UnknownSocket(*unknown));
    }

    let mut found = Vec::new();

    for cpu in read_cpu_list(Path::new(PMU_CPUMASK))
        .map_err(|error| RaplError::Read(PMU_CPUMASK.into(), error))?
    {
        let socket = topology
            .iter()
            .find(|socket| socket.holds(cpu))
            .ok_or(RaplError::CpuPackage(cpu))?
            .id;

        if sockets_spec.is_some_and(|wanted| !wanted.contains(&socket)) {
            continue;
        }

        for (domain, scale) in &available {
            let candidate = RaplDomain {
                domain: *domain,
                socket,
                cpu,
                scale: *scale,
            };

            if is_domain_available(&candidate)? {
                found.push(candidate);
            }
        }
    }

    Ok(found)
}

#[cfg(feature = "backend-perf")]
fn get_domain_scale(domain: RaplDomainType) -> Result<Option<f64>> {
    let mut builder = Dynamic::builder(PMU).map_err(RaplError::Pmu)?;
    if builder.event(domain.to_perf_event()).is_err() {
        return Ok(None);
    }
    builder.scale().map_err(RaplError::Pmu)
}

#[cfg(feature = "backend-perf")]
fn is_domain_available(domain: &RaplDomain) -> Result<bool> {
    let counter = match build_counter(domain) {
        Ok(counter) => counter,
        Err(RaplError::Pmu(error)) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };

    match counter.build() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            Err(RaplError::OpenCounter(domain.name(), error))
        }
        Err(error) => {
            log::debug!("rapl: {} unavailable: {error}", domain.name());
            Ok(false)
        }
    }
}

#[cfg(test)]
mod test {
    use joule_profiler_core::metric::MetricValue;

    use crate::{
        domain::{EnergyUnit, RaplDomainType},
        error::RaplError,
    };

    #[test]
    fn millijoules_and_microjoules_are_rounded_and_joules_are_not() {
        assert_eq!(
            EnergyUnit::Microjoule.value(706_726.4),
            MetricValue::U64(706_726)
        );
        assert_eq!(EnergyUnit::Millijoule.value(8.545), MetricValue::U64(9));
        assert_eq!(
            EnergyUnit::Joule.value(0.706_726),
            MetricValue::F64(0.706_726)
        );
    }

    #[test]
    fn a_domain_is_read_from_any_of_its_names_whatever_their_case() {
        for (name, domain) in [
            ("pp0", RaplDomainType::Core),
            ("energy-gpu", RaplDomainType::Uncore),
            ("PACKAGE", RaplDomainType::Package),
            ("Ram", RaplDomainType::Dram),
            ("platform", RaplDomainType::Psys),
        ] {
            assert_eq!(RaplDomainType::try_from(name).ok(), Some(domain), "{name}");
        }
    }

    #[test]
    fn a_name_that_is_close_to_a_domain_is_still_refused() {
        for name in ["", "cores", "pp2", "energy-core", "energy-package", "sys"] {
            assert!(
                matches!(
                    RaplDomainType::try_from(name),
                    Err(RaplError::UnknownDomain(unknown)) if unknown == name
                ),
                "{name}"
            );
        }
    }
}
