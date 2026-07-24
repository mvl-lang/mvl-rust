#[mvl::total]
fn total_fn() -> i32 {
    1
}

#[mvl::partial]
fn partial_fn() -> i32 {
    2
}

#[mvl::effect(Console)]
fn effect_fn() {}

#[mvl::refine(x > 0)]
fn refine_fn(x: i32) -> i32 {
    x
}

#[mvl::requires(x > 0)]
#[mvl::ensures(result > 0)]
fn contract_fn(x: i32) -> i32 {
    x
}

#[mvl::label(Secret)]
struct Labeled(i32);

#[test]
fn attributes_are_pass_through_and_dont_alter_behavior() {
    assert_eq!(total_fn(), 1);
    assert_eq!(partial_fn(), 2);
    effect_fn();
    assert_eq!(refine_fn(5), 5);
    assert_eq!(contract_fn(5), 5);
    let labeled = Labeled(1);
    assert_eq!(labeled.0, 1);
}

// Recursive termination proof: `n` is the decreases-measure, strictly
// decreasing on each call and bounded below by the `n == 0` base case.
#[mvl::total]
#[mvl::decreases(n)]
fn factorial(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

#[test]
fn decreases_example_is_a_real_recursive_function() {
    assert_eq!(factorial(0), 1);
    assert_eq!(factorial(5), 120);
}

// IFC: taint an untrusted value, then explicitly trust it -- matches
// MVL's `relabel trust(raw, "tag")`.
#[test]
fn tainted_string_requires_explicit_trust_to_declassify() {
    let raw: mvl::Tainted<String> = mvl::Tainted("from the environment".to_string());
    let trusted: String = mvl::trust(raw, "LOG-PATH-001");
    assert_eq!(trusted, "from the environment");
}

#[test]
fn secret_string_requires_explicit_trust_to_declassify() {
    let secret: mvl::Secret<String> = mvl::Secret("hunter2".to_string());
    let revealed: String = mvl::trust(secret, "AUDIT-042");
    assert_eq!(revealed, "hunter2");
}
