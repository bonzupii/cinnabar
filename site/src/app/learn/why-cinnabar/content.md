Cinnabar is built for software where hidden cleanup, suppressed diagnostics, or
runtime failure can become a systems incident.

<!-- @lede -->

Cinnabar assumes a contributor may optimize for finishing the task rather than
for preserving the program’s invariants. The language therefore makes those
invariants part of what can be expressed at all.

## Zero-trust is a language property

Safety is not a lint profile. There is no warning severity, suppression pragma,
or unsafe sublanguage that turns ownership checks off. A program either proves
the required facts or it is rejected with an error.

## The intended domain

The language targets compilers, runtimes, kernels, firmware, and network stacks:
places where garbage collection, invisible control flow, and reachable panic
paths are poor defaults.

## The tradeoff

Cinnabar deliberately gives up escape hatches. The reward is a smaller semantic
surface: resource flow, mutation, impurity, and failure remain visible to the
compiler and to the reader.
