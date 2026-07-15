/// <reference types="vite/client" />

declare module "*.svg?react" {
    import type { FunctionComponent, SVGProps } from "react";

    const src: FunctionComponent<SVGProps<SVGSVGElement>>;
    export default src;
}

// @fontsource packages ship only CSS (no type declarations); side-effect imports
// of their bare specifiers need an ambient module declaration under bundler resolution.
declare module "@fontsource-variable/*";
