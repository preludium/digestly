import { readFile, readdir } from "node:fs/promises";
import { dirname, extname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const sourceRoot = join(repositoryRoot, "web", "src");
const baselinePath = join(repositoryRoot, "scripts", "component-baseline.json");
const sourcePrefix = "web/src/";
const excludedPrefix = "web/src/components/ui/";

async function findTsxFiles(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    const files = [];

    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) {
            files.push(...(await findTsxFiles(path)));
        } else if (entry.isFile() && extname(entry.name) === ".tsx") {
            files.push(path);
        }
    }

    return files;
}

function maskNonCode(source) {
    let masked = "";
    let index = 0;

    while (index < source.length) {
        const character = source[index];
        const next = source[index + 1];

        if (character === "/" && next === "/") {
            masked += "  ";
            index += 2;
            while (index < source.length && source[index] !== "\n") {
                masked += " ";
                index += 1;
            }
            continue;
        }

        if (character === "/" && next === "*") {
            masked += "  ";
            index += 2;
            while (index < source.length) {
                if (source[index] === "*" && source[index + 1] === "/") {
                    masked += "  ";
                    index += 2;
                    break;
                }
                masked += source[index] === "\n" ? "\n" : " ";
                index += 1;
            }
            continue;
        }

        if (character === '"' || character === "'" || character === "`") {
            const quote = character;
            masked += " ";
            index += 1;
            while (index < source.length) {
                if (source[index] === "\\") {
                    masked += " ";
                    index += 1;
                    if (index < source.length) {
                        masked += source[index] === "\n" ? "\n" : " ";
                        index += 1;
                    }
                    continue;
                }
                if (source[index] === quote) {
                    masked += " ";
                    index += 1;
                    break;
                }
                masked += source[index] === "\n" ? "\n" : " ";
                index += 1;
            }
            continue;
        }

        masked += character;
        index += 1;
    }

    return masked;
}

function lineNumber(source, offset) {
    return source.slice(0, offset).split("\n").length;
}

function findComponents(source) {
    const code = maskNonCode(source);
    const components = [];
    const patterns = [
        /\b(?:export\s+(?:default\s+)?)?(?:async\s+)?function\s+([A-Z][A-Za-z0-9_]*)\b/g,
        /\b(?:export\s+(?:default\s+)?)?class\s+([A-Z][A-Za-z0-9_]*)\b/g,
        /\b(?:export\s+)?const\s+([A-Z][A-Za-z0-9_]*)\s*=\s*(?:(?:async\s+)?function\b|(?:React\.)?(?:forwardRef|memo)\b)/g,
        /\b(?:export\s+)?const\s+([A-Z][A-Za-z0-9_]*)\s*=\s*(?:<[^;\n]+>\s*)?(?:\([^;\n]*\)|[A-Za-z_$][\w$]*)\s*(?::\s*[^=;\n]+)?=>/g,
    ];

    for (const pattern of patterns) {
        for (const match of code.matchAll(pattern)) {
            components.push({
                line: lineNumber(source, match.index),
                name: match[1],
            });
        }
    }

    return components.sort((a, b) => a.line - b.line || a.name.localeCompare(b.name));
}

function relativePath(path) {
    return relative(repositoryRoot, path).split(sep).join("/");
}

function formatComponentList(components) {
    return components.map(({ line, name }) => `${name} (line ${line})`).join(", ");
}

async function loadBaseline() {
    const contents = await readFile(baselinePath, "utf8");
    const baseline = JSON.parse(contents);

    if (!baseline || Array.isArray(baseline) || typeof baseline !== "object") {
        throw new Error("component baseline must be a JSON object of file paths to counts");
    }

    for (const [path, count] of Object.entries(baseline)) {
        if (!path.startsWith(sourcePrefix) || path.startsWith(excludedPrefix)) {
            throw new Error(`baseline path is outside the checked source tree: ${path}`);
        }
        if (!Number.isInteger(count) || count <= 1) {
            throw new Error(`baseline count for ${path} must be an integer greater than 1`);
        }
    }

    return baseline;
}

async function main() {
    const baseline = await loadBaseline();
    const files = (await findTsxFiles(sourceRoot)).filter(
        (path) => !relativePath(path).startsWith(excludedPrefix),
    );
    const actual = new Map();

    for (const file of files) {
        const source = await readFile(file, "utf8");
        actual.set(relativePath(file), findComponents(source));
    }

    const failures = [];

    for (const [path, allowed] of Object.entries(baseline)) {
        const components = actual.get(path);
        if (!components) {
            failures.push(`stale baseline entry: ${path} is not an existing checked file`);
            continue;
        }
        if (components.length > allowed) {
            failures.push(
                `${path}: found ${components.length} components, baseline allows ${allowed}: ${formatComponentList(components)}`,
            );
        }
    }

    for (const [path, components] of actual) {
        if (!baseline[path] && components.length > 1) {
            failures.push(
                `${path}: found ${components.length} components, but unlisted files allow at most 1: ${formatComponentList(components)}`,
            );
        }
    }

    if (failures.length > 0) {
        console.error("Component structure validation failed:");
        for (const failure of failures) console.error(`- ${failure}`);
        process.exitCode = 1;
        return;
    }

    console.log(
        `Component structure validation passed: checked ${actual.size} TSX files; ${Object.keys(baseline).length} baseline entries validated.`,
    );
}

try {
    await main();
} catch (error) {
    console.error(`Component structure validation failed: ${error.message}`);
    process.exitCode = 1;
}
