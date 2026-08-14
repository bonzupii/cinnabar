import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";
import { og } from "./(home)/page";

/*
 * The site's default social image, and the home page's.
 *
 * It sits at the root segment rather than inside the (home) route group for
 * two reasons: Next appends a content hash to a metadata image declared inside
 * a group, which would break the stable /opengraph-image path netlify.toml
 * sets the content type for; and at the root it is also the fallback for any
 * future route that does not declare one.
 *
 * The copy it renders lives with the home page's other metadata and is
 * imported here, so a route's title and description have one home rather than
 * two that can disagree.
 */
export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = og.alt;

export default function OpengraphImage() {
  return renderOgImage(og);
}
