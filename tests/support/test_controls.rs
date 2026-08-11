use std::env::VarError;

#[derive(Clone, Copy)]
pub enum TestProfile {
    Full,
    Balanced,
    Smoke,
}

pub fn test_profile() -> TestProfile {
    match std::env::var("CINNABAR_TEST_PROFILE") {
        Ok(value) => match value.as_str() {
            "full" => TestProfile::Full,
            "balanced" => TestProfile::Balanced,
            "smoke" => TestProfile::Smoke,
            invalid => {
                assert!(
                    false,
                    "CINNABAR_TEST_PROFILE must be full, balanced, or smoke; got '{}'",
                    invalid
                );
                TestProfile::Full
            }
        },
        Err(error) => match error {
            VarError::NotPresent => TestProfile::Full,
            VarError::NotUnicode(value) => {
                assert!(false, "CINNABAR_TEST_PROFILE is not Unicode: {:?}", value);
                TestProfile::Full
            }
        },
    }
}

pub fn profile_name(profile: TestProfile) -> &'static str {
    match profile {
        TestProfile::Full => "full",
        TestProfile::Balanced => "balanced",
        TestProfile::Smoke => "smoke",
    }
}

pub fn profile_usize(profile: TestProfile, full: usize, balanced: usize, smoke: usize) -> usize {
    match profile {
        TestProfile::Full => full,
        TestProfile::Balanced => balanced,
        TestProfile::Smoke => smoke,
    }
}

pub fn usize_control(name: &str, default: usize) -> usize {
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

// Selects exactly `budget` cases, spread across the entire ordered corpus,
// rather than taking a prefix that could systematically miss later shapes.
pub fn evenly_selected(index: usize, total: usize, budget: usize) -> bool {
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
