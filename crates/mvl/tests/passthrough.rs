#[mvl::total]
fn total_fn() -> i32 {
    1
}

#[mvl::partial]
fn partial_fn() -> i32 {
    2
}

#[mvl::decreases(0)]
fn decreases_fn() {}

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

#[mvl::declassify]
fn declassify_fn(x: i32) -> i32 {
    x
}

#[test]
fn attributes_are_pass_through_and_dont_alter_behavior() {
    assert_eq!(total_fn(), 1);
    assert_eq!(partial_fn(), 2);
    decreases_fn();
    effect_fn();
    assert_eq!(refine_fn(5), 5);
    assert_eq!(contract_fn(5), 5);
    let labeled = Labeled(1);
    assert_eq!(labeled.0, 1);
    assert_eq!(declassify_fn(5), 5);
}
