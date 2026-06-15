load("@rules_cc//cc:defs.bzl", "cc_binary", "cc_library")
load("@bazel_skylib//lib:paths.bzl", "paths")

def my_cc_library(name, srcs, hdrs = [], deps = []):
    cc_library(
        name = name,
        srcs = srcs,
        hdrs = hdrs,
        deps = deps,
        visibility = ["//visibility:public"],
    )

def my_cc_binary(name, srcs, deps = []):
    cc_binary(
        name = name,
        srcs = srcs,
        deps = deps,
    )

def test_suite_macro(name, tests):
    for t in tests:
        native.sh_test(
            name = t,
            srcs = [t + ".sh"],
        )
    native.test_suite(
        name = name,
        tests = tests,
    )

def _my_rule_impl(ctx):
    out = ctx.actions.declare_file(ctx.label.name + ".out")
    ctx.actions.write(out, ctx.attr.content)
    return [DefaultInfo(files = depset([out]))]

my_rule = rule(
    implementation = _my_rule_impl,
    attrs = {
        "content": attr.string(mandatory = True),
    },
)

def _gen_rule_impl(ctx):
    outs = [ctx.actions.declare_file(s + ".gen") for s in ctx.attr.sources]
    for o in outs:
        ctx.actions.write(o, "generated")
    return [DefaultInfo(files = depset(outs))]

gen_rule = rule(
    implementation = _gen_rule_impl,
    attrs = {"sources": attr.string_list()},
)
