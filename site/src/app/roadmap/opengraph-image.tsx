import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";
import { og } from "./page";

/*
 * Next requires the social image to be its own file, so the copy it renders
 * lives beside the page's other metadata in page.tsx and is imported here —
 * one place to edit a route's title and description rather than two that can
 * disagree.
 */
export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = og.alt;

export default function OpengraphImage() {
  return renderOgImage(og);
}
