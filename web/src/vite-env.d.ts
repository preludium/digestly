/// <reference types="vite/client" />

// @fontsource packages ship only CSS (no type declarations); side-effect imports
// of their bare specifiers need an ambient module declaration under bundler resolution.
declare module "@fontsource-variable/*";
