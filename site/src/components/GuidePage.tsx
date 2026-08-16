import PageHeader from "@/components/PageHeader";
import Markdown from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { ArrowLink, SourceNote } from "@/components/ui";
import { DocIcon } from "@/components/brand/icons";

export default function GuidePage({
  section,
  title,
  lede,
  body,
  source,
  nextHref,
  nextLabel,
}: {
  section: string;
  title: string;
  lede: string;
  body: string;
  source: string;
  nextHref: string;
  nextLabel: string;
}) {
  return <article className="pb-28">
    <PageHeader section={section} note="Learn Cinnabar" icon={DocIcon} title={title} lede={lede} />
    <div className="mx-auto max-w-350 px-6 pt-16 sm:px-10">
      <Reveal className="max-w-[88ch]"><Markdown>{body}</Markdown></Reveal>
      <SourceNote className="mt-16">{source}</SourceNote>
      <ArrowLink href={nextHref} className="mt-12">{nextLabel}</ArrowLink>
    </div>
  </article>;
}
