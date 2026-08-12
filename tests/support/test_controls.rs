use std::env::VarError;

#[derive(Clone, Copy)]
pub(crate) enum TestProfile {
    Full,
    Balanced,
    Smoke,
}

pub(crate) fn test_profile() -> TestProfile {
    let balanced = cfg!(feature = "test-profile-balanced");
    let smoke = cfg!(feature = "test-profile-smoke");
    assert!(
        !(balanced && smoke),
        "test-profile-balanced and test-profile-smoke cannot be enabled together"
    );
    if balanced {
        TestProfile::Balanced
    } else if smoke {
        TestProfile::Smoke
    } else {
        TestProfile::Full
    }
}

pub(crate) fn profile_name(profile: TestProfile) -> &'static str {
    match profile {
        TestProfile::Full => "full",
        TestProfile::Balanced => "balanced",
        TestProfile::Smoke => "smoke",
    }
}

pub(crate) fn profile_usize(
    profile: TestProfile,
    full: usize,
    balanced: usize,
    smoke: usize,
) -> usize {
    match profile {
        TestProfile::Full => full,
        TestProfile::Balanced => balanced,
        TestProfile::Smoke => smoke,
    }
}

pub(crate) fn usize_control(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(value) => match value.parse::<usize>() {
            Ok(parsed) => parsed,
            Err(error) => {
                assert!(false, "{} must be a non-negative integer: {}", name, error);
                default
            }
        },
        Err(error) => match error {
            VarError::NotPresent => default,
            VarError::NotUnicode(value) => {
                assert!(false, "{} is not Unicode: {:?}", name, value);
                default
            }
        },
    }
}

pub(crate) fn reduced_usize_control(
    profile: TestProfile,
    name: &str,
    default: usize,
) -> usize {
    match profile {
        TestProfile::Full => default,
        TestProfile::Balanced => usize_control(name, default),
        TestProfile::Smoke => usize_control(name, default),
    }
}

// Selects exactly `budget` cases, spread across the entire ordered corpus,
// rather than taking a prefix that could systematically miss later shapes.
pub(crate) fn evenly_selected(index: usize, total: usize, budget: usize) -> bool {
    if index >= total {
        return false;
    }
    if budget >= total {
        return true;
    }
    if budget == 0 || total == 0 {
        return false;
    }
    let exact_index = index as u128;
    let exact_budget = budget as u128;
    let exact_total = total as u128;
    let before = exact_index * exact_budget / exact_total;
    let after = (exact_index + 1) * exact_budget / exact_total;
    after > before
}

#[cfg(test)]
mod tests {
    use super::evenly_selected;

    fn selected_indices(total: usize, budget: usize) -> Vec<usize> {
        let mut selected = Vec::new();
        let mut index = 0usize;
        while index < total {
            if evenly_selected(index, total, budget) {
                selected.push(index);
            }
            index += 1;
        }
        selected
    }

    #[test]
    fn zero_budget_selects_nothing() {
        assert_eq!(selected_indices(10, 0), Vec::<usize>::new());
    }

    #[test]
    fn full_budget_selects_every_case() {
        assert_eq!(selected_indices(4, 4), vec![0, 1, 2, 3]);
    }

    #[test]
    fn partial_budget_is_exact_and_spread_across_the_corpus() {
        assert_eq!(selected_indices(10, 3), vec![3, 6, 9]);
    }

    #[test]
    fn out_of_range_index_is_never_selected() {
        assert!(!evenly_selected(10, 10, 10));
    }
}
