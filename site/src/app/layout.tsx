import type { Metadata } from "next";
import { IBM_Plex_Mono, Schibsted_Grotesk } from "next/font/google";
import SiteHeader from "@/components/SiteHeader";
import SiteFooter from "@/components/SiteFooter";
import MushroomEasterEgg from "@/components/MushroomEasterEgg";
import { og } from "./(home)/page";
import { ogImage } from "@/lib/og-image";
import { THEME_INIT_SCRIPT } from "@/lib/theme";
import { DESCRIPTION, SITE_URL } from "@/lib/site";
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
  /*
   * Names the mechanism and the audience, in that order, and nothing else.
   *
   * It read "a zero-trust systems language" — the repository's own framing,
   * but as a page title it is an abstraction a reader has to be told the
   * meaning of before it says anything. "Linear-typed" is checkable, and
   * "compilers and kernels" tells someone in one glance whether this is aimed
   * at them. The full domain list and the stance are in the description below,
   * which has the room for them.
   */
  title: {
    default: "Cinnabar — systems programming without safety escape hatches",
    template: "%s · Cinnabar",
  },
  description: DESCRIPTION,
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
    title: "Cinnabar — systems programming without safety escape hatches",
    description: DESCRIPTION,
    type: "website",
    locale: "en_US",
    url: "/",
    images: [ogImage("/", og)],
  },
  twitter: {
    card: "summary_large_image",
    title: "Cinnabar — systems programming without safety escape hatches",
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
        {/* Renders nothing until the Konami code is typed. */}
        <MushroomEasterEgg />
      </body>
    </html>
  );
}
