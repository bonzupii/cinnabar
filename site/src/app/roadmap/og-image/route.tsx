import { ogImageHandler } from "@/lib/og-image";
import { og } from "../page";

export const dynamic = "force-static";
export const GET = ogImageHandler(og);
