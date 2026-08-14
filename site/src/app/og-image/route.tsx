import { ogImageHandler } from "@/lib/og-image";
import { og } from "../(home)/page";

export const dynamic = "force-static";
export const GET = ogImageHandler(og);
