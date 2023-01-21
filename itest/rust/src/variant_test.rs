/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use crate::itest;
use godot::builtin::{FromVariant, GodotString, StringName, ToVariant, Variant, Vector2, Vector3};
use godot::obj::InstanceId;
use godot::sys::{GodotFfi, VariantOperator, VariantType};
use std::cmp::Ordering;
use std::fmt::{Debug, Display};

pub fn run() -> bool {
    let mut ok = true;
    ok &= variant_nil();
    ok &= variant_conversions();
    ok &= variant_forbidden_conversions();
    ok &= variant_display();
    ok &= variant_get_type();
    ok &= variant_equal();
    ok &= variant_evaluate();
    ok &= variant_evaluate_total_order();
    ok &= variant_sys_conversion();
    ok &= variant_sys_conversion2();
    ok &= variant_object_nullptr();
    ok
}

#[itest]
fn variant_nil() {
    let variant = Variant::nil();
    assert!(variant.is_nil());

    let variant = 12345i32.to_variant();
    assert!(!variant.is_nil());
}

#[itest]
fn variant_conversions() {
    roundtrip(false);
    roundtrip(true);
    roundtrip(InstanceId::from_nonzero(-9223372036854775808i64));
    // roundtrip(Some(InstanceId::from_nonzero(9223372036854775807i64)));
    // roundtrip(Option::<InstanceId>::None);

    // unsigned
    roundtrip(0u8);
    roundtrip(255u8);
    roundtrip(0u16);
    roundtrip(65535u16);
    roundtrip(0u32);
    roundtrip(4294967295u32);

    // signed
    roundtrip(127i8);
    roundtrip(-128i8);
    roundtrip(32767i16);
    roundtrip(-32768i16);
    roundtrip(2147483647i32);
    roundtrip(-2147483648i32);
    roundtrip(9223372036854775807i64);

    // string
    roundtrip(gstr("some string"));
    roundtrip(String::from("some other string"));
    let str_val = "abcdefghijklmnop";
    let back = String::from_variant(&str_val.to_variant());
    assert_eq!(str_val, back.as_str());
}

#[itest]
fn variant_forbidden_conversions() {
    truncate_bad::<i8>(128);
}

#[itest]
fn variant_get_type() {
    let variant = Variant::nil();
    assert_eq!(variant.get_type(), VariantType::Nil);

    let variant = 74i32.to_variant();
    assert_eq!(variant.get_type(), VariantType::Int);

    let variant = true.to_variant();
    assert_eq!(variant.get_type(), VariantType::Bool);

    let variant = gstr("hello").to_variant();
    assert_eq!(variant.get_type(), VariantType::String);
}

#[itest]
fn variant_equal() {
    assert_eq!(Variant::nil(), ().to_variant());
    assert_eq!(Variant::nil(), Variant::default());
    assert_eq!(Variant::from(77), 77.to_variant());

    equal(77, (), false);
    equal(77, 78, false);

    assert_ne!(77.to_variant(), Variant::nil());
    assert_ne!(77.to_variant(), 78.to_variant());

    //equal(77, 77.0, false)
    equal(Vector3::new(1.0, 2.0, 3.0), Vector2::new(1.0, 2.0), false);
    equal(1, true, false);
    equal(false, 0, false);
    equal(gstr("String"), 33, false);
}

#[rustfmt::skip]
#[itest]
fn variant_evaluate() {
    evaluate(VariantOperator::Add, 20, -39, -19);
    evaluate(VariantOperator::Greater, 20, 19, true);
    evaluate(VariantOperator::Equal, 20, 20.0, true);
    evaluate(VariantOperator::NotEqual, 20, 20.0, false);
    evaluate(VariantOperator::Multiply, 5, 2.5, 12.5);

    evaluate(VariantOperator::Equal, gstr("hello"), gstr("hello"), true);
    evaluate(VariantOperator::Equal, gstr("hello"), gname("hello"), true);
    evaluate(VariantOperator::Equal, gname("rust"), gstr("rust"), true);
    evaluate(VariantOperator::Equal, gname("rust"), gname("rust"), true);

    evaluate(VariantOperator::NotEqual, gstr("hello"), gstr("hallo"), true);
    evaluate(VariantOperator::NotEqual, gstr("hello"), gname("hallo"), true);
    evaluate(VariantOperator::NotEqual, gname("rust"), gstr("rest"), true);
    evaluate(VariantOperator::NotEqual, gname("rust"), gname("rest"), true);

    evaluate_fail(VariantOperator::Equal, 1, true);
    evaluate_fail(VariantOperator::Equal, 0, false);
    evaluate_fail(VariantOperator::Subtract, 2, Vector3::new(1.0, 2.0, 3.0));
}

#[itest]
fn variant_evaluate_total_order() {
    // See also Godot 4 source: variant_op.cpp

    // NaN incorrect in Godot
    // use VariantOperator::{Equal, Greater, GreaterEqual, Less, LessEqual, NotEqual};
    // for op in [Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual] {
    //     evaluate(op, f64::NAN, f64::NAN, false);
    // }

    total_order(-5, -4, Ordering::Less);
    total_order(-5, -4.0, Ordering::Less);
    total_order(-5.0, -4, Ordering::Less);

    total_order(-5, -5, Ordering::Equal);
    total_order(-5, -5.0, Ordering::Equal);
    total_order(-5.0, -5, Ordering::Equal);

    total_order(gstr("hello"), gstr("hello"), Ordering::Equal);
    total_order(gstr("hello"), gstr("hell"), Ordering::Greater);
}

#[itest]
fn variant_display() {
    let cases = [
        (Variant::nil(), "<null>"),
        (false.to_variant(), "false"),
        (true.to_variant(), "true"),
        (gstr("some string").to_variant(), "some string"),
        //
        // unsigned
        ((0u8).to_variant(), "0"),
        ((255u8).to_variant(), "255"),
        ((0u16).to_variant(), "0"),
        ((65535u16).to_variant(), "65535"),
        ((0u32).to_variant(), "0"),
        ((4294967295u32).to_variant(), "4294967295"),
        //
        // signed
        ((127i8).to_variant(), "127"),
        ((-128i8).to_variant(), "-128"),
        ((32767i16).to_variant(), "32767"),
        ((-32768i16).to_variant(), "-32768"),
        ((2147483647i32).to_variant(), "2147483647"),
        ((-2147483648i32).to_variant(), "-2147483648"),
        ((9223372036854775807i64).to_variant(), "9223372036854775807"),
        (
            (-9223372036854775808i64).to_variant(),
            "-9223372036854775808",
        ),
    ];

    for (variant, string) in cases {
        assert_eq!(&variant.to_string(), string);
    }
}

#[itest]
fn variant_sys_conversion() {
    let v = Variant::from(7);
    let ptr = v.sys();

    let v2 = unsafe { Variant::from_sys(ptr) };
    assert_eq!(v2, v);
}

#[itest]
fn variant_object_nullptr() {
    // Some Variants that are of type Object actually contain a null pointer.
    // The should report their type as Nil instead, so that they don't create invalid Gd<T> objects.
    let variant;
    unsafe {
        // Create a variant from a null object pointer.
        variant = Variant::from_var_sys_init(|self_ptr| {
            let null_pointer: godot::sys::GDExtensionObjectPtr = std::ptr::null_mut();
            let converter = godot::sys::builtin_fn!(object_to_variant);
            converter(
                self_ptr,
                std::ptr::addr_of!(null_pointer) as godot::sys::GDExtensionTypePtr,
            );
        });

        // Get the variant's internal type and ensure that it is actually still an Object.
        let sys_type: godot::sys::GDExtensionVariantType =
            godot::sys::interface_fn!(variant_get_type)(variant.var_sys());
        assert_eq!(sys_type, godot::sys::GDEXTENSION_VARIANT_TYPE_OBJECT);
    }

    // The actual test of the public API, ensure that the variant's type is Nil even though the internal type is Object.
    assert_eq!(variant.get_type(), VariantType::Nil);
}

#[itest]
fn variant_sys_conversion2() {
    /*
    let buffer = [0u8; 50];

    let v = Variant::from(7);
    unsafe { v.write_sys(buffer.as_mut_ptr()) };

    let v2 = unsafe {
        Variant::from_sys_init(|ptr| {
            std::ptr::copy(buffer.as_ptr(), ptr as *mut u8, std::mem::size_of_val(*ptr))
        })
    };
    assert_eq!(v2, v);
    */
}

// ----------------------------------------------------------------------------------------------------------------------------------------------

fn roundtrip<T>(value: T)
where
    T: FromVariant + ToVariant + PartialEq + Debug,
{
    // TODO test other roundtrip (first FromVariant, then ToVariant)
    // Some values can be represented in Variant, but not in T (e.g. Variant(0i64) -> Option<InstanceId> -> Variant is lossy)

    let variant = value.to_variant();
    let back = T::try_from_variant(&variant).unwrap();

    assert_eq!(value, back);
}

fn truncate_bad<T>(original_value: i64)
where
    T: FromVariant + Display,
{
    let variant = original_value.to_variant();
    let result = T::try_from_variant(&variant);

    if let Ok(back) = result {
        panic!(
            "{} - T::try_from_variant({}) should fail, but resulted in {}",
            std::any::type_name::<T>(),
            variant,
            back
        );
    }
}

fn equal<T, U>(lhs: T, rhs: U, expected: bool)
where
    T: ToVariant,
    U: ToVariant,
{
    if expected {
        assert_eq!(lhs.to_variant(), rhs.to_variant());
    } else {
        assert_ne!(lhs.to_variant(), rhs.to_variant());
    }
}

fn evaluate<T, U, E>(op: VariantOperator, lhs: T, rhs: U, expected: E)
where
    T: ToVariant,
    U: ToVariant,
    E: ToVariant,
{
    let lhs = lhs.to_variant();
    let rhs = rhs.to_variant();
    let expected = expected.to_variant();

    assert_eq!(lhs.evaluate(&rhs, op), Some(expected));
}

fn evaluate_fail<T, U>(op: VariantOperator, lhs: T, rhs: U)
where
    T: ToVariant,
    U: ToVariant,
{
    let lhs = lhs.to_variant();
    let rhs = rhs.to_variant();

    assert_eq!(lhs.evaluate(&rhs, op), None);
}

fn total_order<T, U>(lhs: T, rhs: U, expected_order: Ordering)
where
    T: ToVariant,
    U: ToVariant,
{
    fn eval(v: Option<Variant>) -> bool {
        v.expect("comparison is valid").to::<bool>()
    }

    let lhs = lhs.to_variant();
    let rhs = rhs.to_variant();

    let eq = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::Equal));
    let ne = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::NotEqual));
    let lt = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::Less));
    let le = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::LessEqual));
    let gt = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::Greater));
    let ge = eval(Variant::evaluate(&lhs, &rhs, VariantOperator::GreaterEqual));

    let true_rels;
    let false_rels;

    match expected_order {
        Ordering::Less => {
            true_rels = [ne, lt, le];
            false_rels = [eq, gt, ge];
        }
        Ordering::Equal => {
            true_rels = [eq, le, ge];
            false_rels = [ne, lt, gt];
        }
        Ordering::Greater => {
            true_rels = [ne, gt, ge];
            false_rels = [eq, lt, le];
        }
    }

    for rel in true_rels {
        assert!(
            rel,
            "total_order(rel=true, lhs={lhs:?}, rhs={rhs:?}, exp={expected_order:?})",
        );
    }
    for rel in false_rels {
        assert!(
            !rel,
            "total_order(rel=false, lhs={lhs:?}, rhs={rhs:?}, exp={expected_order:?})",
        );
    }
}

fn gstr(s: &str) -> GodotString {
    GodotString::from(s)
}

fn gname(s: &str) -> StringName {
    StringName::from(s)
}
