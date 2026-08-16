import type { ComponentType } from "react";
import { BorrowIcon, CheckIcon, DocIcon, LinearIcon, RunIcon } from "@/components/brand/icons";

export type LearnChapter = {
  href: string;
  title: string;
  summary: string;
  icon: ComponentType<{ size?: number; className?: string }>;
};

export const LEARN_CHAPTERS: readonly LearnChapter[] = [
  { href: "/learn/why-cinnabar/", title: "Why Cinnabar", summary: "The failure modes and design stance that shape the language.", icon: DocIcon },
  { href: "/learn/linear-types/", title: "Linear types", summary: "How resource handles acquire an exactly-once consumption obligation.", icon: LinearIcon },
  { href: "/learn/borrowing/", title: "Borrowing", summary: "Shared and exclusive references with scopes inferred from control flow.", icon: BorrowIcon },
  { href: "/learn/error-handling/", title: "Error handling", summary: "Failure is a value to match or propagate, never a hidden panic path.", icon: CheckIcon },
  { href: "/learn/first-program/", title: "First program", summary: "Enter the toolchain, create a project, and run the front end.", icon: RunIcon },
] as const;
