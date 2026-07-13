import ReactMarkdown from "react-markdown";
import { cn } from "@/lib/utils";

/** Renders AI-generated summary text as markdown (bold, lists, paragraphs) using the same
 *  typography as real article HTML (`.article-content` in index.css). Summaries come back from
 *  the LLM as markdown, not plain text - rendering it verbatim showed literal "**bold**". */
export function Markdown({
    children,
    className,
}: {
    children: string;
    className?: string;
}) {
    return (
        <div className={cn("article-content", className)}>
            <ReactMarkdown>{children}</ReactMarkdown>
        </div>
    );
}
