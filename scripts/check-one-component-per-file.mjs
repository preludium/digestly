import { execFileSync } from "node:child_process";
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

        if (character === "/" && isRegexLiteralStart(source, index)) {
            masked += " ";
            index += 1;
            let inCharacterClass = false;

            while (index < source.length) {
                if (source[index] === "\\") {
                    masked += "  ";
                    index += 2;
                    continue;
                }
                if (source[index] === "\n" || source[index] === "\r") {
                    masked += source[index];
                    index += 1;
                    break;
                }
                if (source[index] === "[") inCharacterClass = true;
                if (source[index] === "]") inCharacterClass = false;
                if (source[index] === "/" && !inCharacterClass) {
                    masked += " ";
                    index += 1;
                    while (/[A-Za-z]/.test(source[index] ?? "")) {
                        masked += " ";
                        index += 1;
                    }
                    break;
                }
                masked += " ";
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

function isRegexLiteralStart(source, offset) {
    const prefix = source.slice(0, offset).trimEnd();
    const previousCharacter = prefix.at(-1);
    if (!previousCharacter) return true;
    if ("([{=,:;!?&|+-*%^~".includes(previousCharacter)) return true;
    if (prefix.endsWith("=>")) return true;

    const previousWord = prefix.match(/(?:^|[^A-Za-z0-9_$])([A-Za-z_$][\w$]*)$/)?.[1];
    return [
        "await",
        "case",
        "delete",
        "do",
        "else",
        "in",
        "instanceof",
        "of",
        "return",
        "throw",
        "typeof",
        "void",
        "yield",
    ].includes(previousWord);
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
        /\b(?:export\s+)?const\s+([A-Z][A-Za-z0-9_]*)\s*(?::\s*[^=]*?)?=\s*(?:(?:async\s+)?function\b|(?:React\.)?(?:forwardRef|memo)\b)/g,
        /\b(?:export\s+)?const\s+([A-Z][A-Za-z0-9_]*)\s*(?::\s*[^=]*?)?=\s*(?:async\s+)?(?:<[^;]*>\s*)?(?:\([^)]*?\)|[A-Za-z_$][\w$]*)\s*(?::\s*[^=]*?)?\s*=>/g,
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

function validateBaseline(baseline, description) {
    if (!baseline || Array.isArray(baseline) || typeof baseline !== "object") {
        throw new Error(`${description} must be a JSON object of file paths to counts`);
    }

    for (const [path, count] of Object.entries(baseline)) {
        if (!path.startsWith(sourcePrefix) || path.startsWith(excludedPrefix)) {
            throw new Error(`${description} path is outside the checked source tree: ${path}`);
        }
        if (!Number.isInteger(count) || count < 1) {
            throw new Error(`${description} count for ${path} must be a positive integer`);
        }
    }

    return baseline;
}

async function loadBaseline() {
    const contents = await readFile(baselinePath, "utf8");
    return validateBaseline(JSON.parse(contents), "component baseline");
}

function readRevisionFile(revision, path) {
    try {
        return execFileSync("git", ["show", `${revision}:${path}`], {
            encoding: "utf8",
            stdio: ["ignore", "pipe", "ignore"],
        });
    } catch {
        return null;
    }
}

function checkBaselineRatchet(baseline, baseSha) {
    if (!baseSha) return [];

    try {
        execFileSync("git", ["rev-parse", "--verify", `${baseSha}^{commit}`], {
            stdio: ["ignore", "pipe", "ignore"],
        });
    } catch {
        throw new Error(`BASE_SHA does not identify an available git revision: ${baseSha}`);
    }

    const baseContents = readRevisionFile(baseSha, "scripts/component-baseline.json");
    const failures = [];

    if (baseContents !== null) {
        const baseBaseline = validateBaseline(
            JSON.parse(baseContents),
            "base component baseline",
        );
        for (const [path, count] of Object.entries(baseline)) {
            if (!(path in baseBaseline)) {
                failures.push(`${path}: new component baseline entries are not allowed`);
            } else if (count > baseBaseline[path]) {
                failures.push(
                    `${path}: component baseline allowance increased (${baseBaseline[path]} -> ${count})`,
                );
            }
        }
        return failures;
    }

    for (const [path, count] of Object.entries(baseline)) {
        const baseSource = readRevisionFile(baseSha, path);
        if (baseSource === null) {
            if (count > 1) {
                failures.push(
                    `${path}: a new file may not be added to the bootstrap baseline above 1 (got ${count})`,
                );
            }
            continue;
        }

        const baseCount = findComponents(baseSource).length;
        if (count > baseCount) {
            failures.push(
                `${path}: bootstrap allowance ${count} exceeds the base component count ${baseCount}`,
            );
        }
    }

    return failures;
}

async function main() {
    const baseline = await loadBaseline();
    const ratchetFailures = checkBaselineRatchet(baseline, process.env.BASE_SHA?.trim());
    const files = (await findTsxFiles(sourceRoot)).filter(
        (path) => !relativePath(path).startsWith(excludedPrefix),
    );
    const actual = new Map();

    for (const file of files) {
        const source = await readFile(file, "utf8");
        actual.set(relativePath(file), findComponents(source));
    }

    const failures = [...ratchetFailures];

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
