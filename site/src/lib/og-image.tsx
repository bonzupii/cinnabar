import { OG_CONTENT_TYPE, OG_SIZE } from "@/lib/constants";
import { renderOgImage } from "@/lib/og-template";

/*
 * Social images, as ordinary route handlers at `<route>/og-image`.
 *
 * Next has a metadata convention for this, but it requires a file named
 * `opengraph-image.tsx` in every segment and emits it at a path with no
 * extension. A route handler is named for what it serves, is written once and
 * reused, and lets the metadata point at a URL this code chooses — so
 * netlify.toml can set the content type on a path that will not move.
 *
 * The copy each image renders lives with its page's other metadata and is
 * passed in here, so a route's title and description have one home.
 */

export type OgCopy = {
  /** Small uppercase label above the title. */
  eyebrow: string;
  title: string;
  description: string;
  /** Alternative text for the rendered card. */
  alt: string;
};

/** The GET handler a route's `og-image/route.tsx` exports. */
export function ogImageHandler(copy: OgCopy) {
  return () => renderOgImage(copy);
}

/** The image descriptor for a route, for use inside an existing metadata block. */
export function ogImage(route: string, copy: OgCopy) {
  return {
    url: route === "/" ? "/og-image" : `${route}og-image`,
    width: OG_SIZE.width,
    height: OG_SIZE.height,
    alt: copy.alt,
  };
}

/**
 * The `openGraph`/`twitter` image metadata for a route.
 *
 * The file convention would inject this; since the image is a route handler,
 * each page states it — which is also what makes the URL predictable.
 *
 * Spread this only into a metadata object that does not already declare
 * `openGraph` or `twitter` of its own, or it will replace them; where one
 * exists, use `ogImage` inside it instead.
 */
export function ogImageMetadata(route: string, copy: OgCopy) {
  const image = ogImage(route, copy);
  return {
    openGraph: { images: [image] },
    twitter: { images: [image] },
  };
}

export { OG_CONTENT_TYPE, OG_SIZE };
