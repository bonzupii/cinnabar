import {
  BorrowIcon,
  CheckIcon,
  CodegenIcon,
  DiagnosticIcon,
  FmtIcon,
  LinearIcon,
  RunIcon,
  StaticLinkIcon,
} from "@/components/brand/icons";

/*
 * The icon each language highlight is drawn with.
 *
 * The highlights themselves — their titles and their wording, which is
 * README.md's own — are the `@highlights` block of src/app/(home)/content.md,
 * one `###` section each. Only the pairing is decided here, and each pairing
 * uses the plate 07 icon whose meaning actually matches the claim.
 *
 * The key is the slug of the heading, so a reworded title fails the build
 * rather than quietly losing its icon. tests/unit/content-bindings.test.ts
 * checks that the two files agree without a page having to be rendered.
 */

export const HIGHLIGHT_ICONS: Record<string, typeof LinearIcon> = {
  "linear-resource-management": LinearIcon,
  "no-lifetime-annotations": BorrowIcon,
  "no-dereference-operator": FmtIcon,
  "errors-only-never-warnings": DiagnosticIcon,
  "no-panics-reachable-from-user-code": CheckIcon,
  "o1-call-stack-recursion": RunIcon,
  "explicit-everything": CodegenIcon,
  "static-freestanding-binaries": StaticLinkIcon,
};
