import type { Metadata } from "next";
import { IBM_Plex_Mono, Schibsted_Grotesk } from "next/font/google";
import SiteHeader from "@/components/SiteHeader";
import SiteFooter from "@/components/SiteFooter";
import { og } from "./(home)/page";
import { ogImage } from "@/lib/og-image";
import { THEME_INIT_SCRIPT } from "@/lib/theme";
import { DESCRIPTION, SITE_URL, TAGLINE } from "@/lib/site";
import "./globals.css";

/*
 * Plate 06 fixes the pairing: Schibsted Grotesk for everything the reader
 * reads as prose, IBM Plex Mono for every technical string — flags, paths,
 * types, versions, diagnostics. The grotesk never sets code.
 */
const schibstedGrotesk = Schibsted_Grotesk({
  variable: "--font-schibsted-grotesk",
  subsets: ["latin"],
  weight: ["400", "500", "700", "800"],
  display: "swap",
});

const ibmPlexMono = IBM_Plex_Mono({
  variable: "--font-ibm-plex-mono",
  subsets: ["latin"],
  weight: ["300", "400", "500", "600"],
  style: ["normal", "italic"],
  display: "swap",
});


export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: "Cinnabar — a zero-trust systems language",
    template: "%s · Cinnabar",
  },
  description: TAGLINE,
  applicationName: "Cinnabar",
  alternates: { canonical: "/" },
  keywords: [
    "Cinnabar",
    "systems programming language",
    "linear types",
    "Austral",
    "borrow checker",
    "LLVM",
    "Rust alternative",
    "freestanding",
    "musl",
  ],
  openGraph: {
    siteName: "Cinnabar",
    title: "Cinnabar — a zero-trust systems language",
    description: DESCRIPTION,
    type: "website",
    locale: "en_US",
    url: "/",
    images: [ogImage("/", og)],
  },
  twitter: {
    card: "summary_large_image",
    title: "Cinnabar — a zero-trust systems language",
    description: DESCRIPTION,
    images: [ogImage("/", og)],
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      className={`${schibstedGrotesk.variable} ${ibmPlexMono.variable} h-full`}
    >
      <body className="bg-ground text-text flex min-h-full flex-col">
        {/*
          Applies a stored theme before first paint, so a visitor who chose a
          theme never sees the other one first.

          A plain inline script, not next/script: `beforeInteractive` is
          dropped entirely from a static export, which put the whole thing back
          to a flash on every load. As the first node in <body> this runs
          before any of the page below it is painted.
        */}
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
        <SiteHeader />
        <main id="main-content" tabIndex={-1} className="flex-1 outline-none">
          {children}
        </main>
        <SiteFooter />
      </body>
    </html>
  );
}
