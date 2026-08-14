import type { MetadataRoute } from "next";
import { ROUTES, SITE_URL } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const lastModified = new Date();
  return ROUTES.map((route) => ({
    // ROUTES already carry their trailing slash, and SITE_URL carries none, so
    // the join never doubles a separator.
    url: route === "/" ? `${SITE_URL}/` : `${SITE_URL}${route}`,
    lastModified,
  }));
}
