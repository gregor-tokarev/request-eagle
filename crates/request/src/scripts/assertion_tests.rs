use std::sync::{Arc, atomic::AtomicBool};

use super::{RequestScripts, runtime::pre_request};
use crate::HttpRequest;

fn check(cases: &[(&str, bool)]) {
    let source = cases
        .iter()
        .enumerate()
        .map(|(index, (source, _))| format!("pm.test('{index}', () => {{ {source} }});\n"))
        .collect();
    let request = HttpRequest {
        scripts: RequestScripts {
            pre_request: source,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, _, reports) =
        smol::block_on(pre_request(request, Arc::new(AtomicBool::new(false)))).unwrap();

    assert_eq!(reports[0].tests.len(), cases.len());
    for (test, (source, pass)) in reports[0].tests.iter().zip(cases) {
        assert_eq!(test.error.is_none(), *pass, "{source}: {:?}", test.error);
    }
}

#[test]
fn comparisons_preserve_types_negation_and_property_targets() {
    check(&[
        ("pm.expect(0).to.eq(-0)", true),
        ("pm.expect(NaN).to.eql(NaN)", true),
        ("pm.expect(0).not.to.deep.equal(-0)", true),
        ("pm.expect({a: 1, b: 2}).to.eql({b: 2, a: 1})", true),
        ("pm.expect({a: undefined}).to.eql({})", false),
        ("pm.expect({a: 1}).to.equal({a: 1})", false),
        ("pm.expect({a: 1}).to.deep.equal({a: '1'})", false),
        ("pm.expect([,]).to.eql([undefined])", true),
        ("pm.expect(new Date(42)).to.eql(new Date(42))", true),
        ("pm.expect(/ok/i).not.to.eql(/ok/g)", true),
        (
            "const shared = {child: {}}; pm.expect({child: shared}).not.eql(shared)",
            true,
        ),
        ("pm.expect(2).not.not.equal(2)", false),
        (
            "pm.expect({a: 1}).to.have.property('a').that.equals(1)",
            true,
        ),
        (
            "pm.expect({a: undefined}).to.have.property('a', undefined)",
            true,
        ),
        ("pm.expect({}).to.have.property('a', undefined)", false),
        ("pm.expect({}).not.to.have.property('a', 1)", true),
        (
            "pm.expect(Object.create({a: 1})).to.have.property('a', 1)",
            true,
        ),
        (
            "pm.expect(Object.create({a: 1})).not.to.have.own.property('a')",
            true,
        ),
        (
            r"pm.expect({'a.b': {'[x]': 2}}).to.have.nested.property('a\\.b.\\[x\\]', 2)",
            true,
        ),
        (
            "pm.expect({a: null}).not.to.have.nested.property('a.b')",
            true,
        ),
        ("pm.expect({a: null}).to.have.nested.property('a.b')", false),
    ]);
}

#[test]
fn collection_assertions_distinguish_exact_subset_and_ordered_matches() {
    check(&[
        ("pm.expect({a: 1, b: 2}).to.have.keys(['a', 'b'])", true),
        ("pm.expect({a: 1, b: 2}).not.to.have.keys('a')", true),
        ("pm.expect({a: 1, b: 2}).to.include.keys('a')", true),
        ("pm.expect({a: 1, b: 2}).not.to.include.keys('a')", false),
        ("pm.expect({a: 1}).to.have.any.keys('a', 'b')", true),
        ("pm.expect({a: 1}).to.have.any.all.keys('a', 'b')", false),
        ("pm.expect([1, 1, 2]).to.have.members([2, 1, 1])", true),
        ("pm.expect([1, 1, 2]).to.have.members([2, 2, 1])", false),
        ("pm.expect([1, 2]).to.include.members([1, 1])", true),
        (
            "pm.expect([1, 2, 3]).to.include.ordered.members([1, 2])",
            true,
        ),
        (
            "pm.expect([1, 2, 3]).to.include.ordered.members([2, 3])",
            false,
        ),
        ("pm.expect([1, 2]).to.have.ordered.members([2, 1])", false),
        ("pm.expect([{a: 1}]).to.have.members([{a: 1}])", false),
        ("pm.expect([{a: 1}]).to.have.deep.members([{a: 1}])", true),
        ("pm.expect({a: {b: 1}}).to.nested.include({'a.b': 1})", true),
        ("pm.expect({a: 1, b: 2}).not.include({a: 1, b: 3})", true),
        ("pm.expect({a: {b: 1}}).to.deep.contain({a: {b: 1}})", true),
        ("pm.expect([1, 2]).to.include.oneOf([2, 3])", true),
        ("pm.expect({a: 1}).to.deep.oneOf([{a: 1}])", true),
        ("pm.expect('Eagle').to.contain('agl').and.match(/^E/)", true),
    ]);
}

#[test]
fn scalar_assertions_and_aliases_work_with_chained_lengths() {
    check(&[
        (
            "pm.expect([1, 2]).to.be.an('array').that.has.length(2)",
            true,
        ),
        ("pm.expect({a: 1}).to.have.a.property('a')", true),
        (
            "pm.expect([1, 2]).to.have.lengthOf.above(1).and.below(3)",
            true,
        ),
        ("pm.expect('abc').to.have.length.within(2, 4)", true),
        (
            "pm.expect(3).to.be.greaterThan(2).and.lessThan(4).and.gte(3).and.lte(3)",
            true,
        ),
        ("pm.expect(3).to.be.closeTo(3.1, 0.2)", true),
        ("pm.expect(3).to.be.closeTo(3.1, 0.01)", false),
        ("pm.expect(3).to.be.within(1, 3)", true),
        ("pm.expect(3).not.to.be.within(1, 3)", false),
        ("pm.expect(3).to.be.finite", true),
        ("pm.expect(Infinity).to.be.finite", false),
        ("pm.expect(NaN).to.be.NaN", true),
        ("pm.expect('NaN').to.be.NaN", false),
        ("pm.expect(0).to.exists.and.not.be.ok", true),
        ("pm.expect(null).to.be.null.and.not.exist", true),
        ("pm.expect(undefined).to.be.undefined", true),
        ("pm.expect(false).to.be.false", true),
        ("pm.expect(1).to.be.true", false),
        ("pm.expect({}).to.be.empty", true),
        ("pm.expect({a: undefined}).to.be.empty", false),
    ]);
}

#[test]
fn unsupported_assertions_and_invalid_arguments_cannot_pass_negated_tests() {
    let cases = [
        "pm.expect(true).to.be.tru",
        "pm.expect(true).not.to.be.constructor",
        "pm.expect(true).equal.to.be.true",
        "pm.expect(() => {}).not.to.throw()",
        "pm.expect(1).not.to.satisfy(() => false)",
        "pm.expect(1).not.within(2, '3')",
        "pm.expect(1).not.above('2')",
        "pm.expect(1).not.closeTo(2, -1)",
        "pm.expect(1).not.lengthOf(3)",
        "pm.expect(1).not.oneOf(2)",
        "pm.expect('abc').not.match('x')",
        "pm.expect(2).not.empty",
        "pm.expect({}).not.include({})",
        "pm.expect({a: 1}).not.keys('a', 'a')",
        "pm.expect({a: 1}).not.nested.own.property('a')",
        "pm.expect({a: 1}).not.nested.property('a..b')",
        "pm.expect({a: 1}).not.nested.property('a[')",
        "pm.expect({a: 1}).not.nested.property('a[x]')",
        "pm.expect(new Map()).not.deep.equal(new Map([[1, 2]]))",
        "pm.expect(new Set()).not.include(1)",
        "const a = {}, b = {}; a.self = a; b.self = b; pm.expect(a).not.eql(b)",
    ];
    check(&cases.map(|source| (source, false)));
}

#[test]
fn assertion_errors_retain_custom_messages_and_useful_values() {
    check(&[
        (
            "try { pm.expect(201, 'status mismatch').equal(200); } catch (e) { if (!e.message.includes('status mismatch') || !e.message.includes('201') || !e.message.includes('200')) throw e; return; } throw Error('assertion did not fail')",
            true,
        ),
        (
            "try { pm.expect('x').a('number', 'wrong type'); } catch (e) { if (!e.message.includes('wrong type')) throw e; return; } throw Error('assertion did not fail')",
            true,
        ),
    ]);
}
