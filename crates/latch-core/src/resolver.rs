use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteIntent {
    WebInteraction,
    DesktopInteraction,
    ApplicationControl,
    NativeSystemControl,
    McpOperation,
    InteractiveTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderRoute {
    Playwright,
    WindowsNative,
    WindowsUia,
    ExplicitMcp,
    ConPty,
    RawComputer,
}

impl ProviderRoute {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Playwright => "playwright",
            Self::WindowsNative => "windows_native",
            Self::WindowsUia => "windows_uia",
            Self::ExplicitMcp => "explicit_mcp",
            Self::ConPty => "conpty",
            Self::RawComputer => "raw_computer",
        }
    }
}

impl fmt::Display for ProviderRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteCandidate {
    pub route: ProviderRoute,
    pub available: bool,
    pub permitted: bool,
    pub priority: u16,
    pub deterministic_verification: bool,
}

impl RouteCandidate {
    pub const fn new(
        route: ProviderRoute,
        available: bool,
        permitted: bool,
        priority: u16,
        deterministic_verification: bool,
    ) -> Self {
        Self {
            route,
            available,
            permitted,
            priority,
            deterministic_verification,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteResolution {
    pub intent: RouteIntent,
    pub route: ProviderRoute,
    pub deterministic_verification: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    Unavailable(RouteIntent),
    PermissionDenied {
        intent: RouteIntent,
        route: ProviderRoute,
    },
}

pub fn resolve_route(
    intent: RouteIntent,
    candidates: &[RouteCandidate],
) -> Result<RouteResolution, ResolveError> {
    let strongest = candidates
        .iter()
        .filter(|candidate| candidate.available)
        .max_by_key(|candidate| candidate.priority)
        .copied()
        .ok_or(ResolveError::Unavailable(intent))?;

    // A locally denied strongest available route is terminal. The resolver never
    // sneaks through a weaker control surface merely to bypass policy.
    if !strongest.permitted {
        return Err(ResolveError::PermissionDenied {
            intent,
            route: strongest.route,
        });
    }

    Ok(RouteResolution {
        intent,
        route: strongest.route,
        deterministic_verification: strongest.deterministic_verification,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_prefers_playwright_over_uia_and_pixels() {
        let resolved = resolve_route(
            RouteIntent::WebInteraction,
            &[
                RouteCandidate::new(ProviderRoute::RawComputer, true, true, 10, false),
                RouteCandidate::new(ProviderRoute::WindowsUia, true, true, 50, true),
                RouteCandidate::new(ProviderRoute::Playwright, true, true, 100, true),
            ],
        )
        .unwrap();
        assert_eq!(resolved.route, ProviderRoute::Playwright);
    }

    #[test]
    fn unavailable_stronger_provider_allows_known_fallback() {
        let resolved = resolve_route(
            RouteIntent::ApplicationControl,
            &[
                RouteCandidate::new(ProviderRoute::RawComputer, true, true, 10, false),
                RouteCandidate::new(ProviderRoute::WindowsUia, true, true, 50, true),
                RouteCandidate::new(ProviderRoute::WindowsNative, false, true, 100, true),
            ],
        )
        .unwrap();
        assert_eq!(resolved.route, ProviderRoute::WindowsUia);
    }

    #[test]
    fn denial_does_not_fall_through_to_weaker_route() {
        let error = resolve_route(
            RouteIntent::DesktopInteraction,
            &[
                RouteCandidate::new(ProviderRoute::RawComputer, true, true, 10, false),
                RouteCandidate::new(ProviderRoute::WindowsUia, true, false, 100, true),
            ],
        )
        .unwrap_err();
        assert_eq!(
            error,
            ResolveError::PermissionDenied {
                intent: RouteIntent::DesktopInteraction,
                route: ProviderRoute::WindowsUia,
            }
        );
    }
}
