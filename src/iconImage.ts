// Custom icon from a file: rasterized to a 32 × 32 PNG in the frontend. SVG is only loaded as an
// image (scripts do not run in <img>) and rasterized; markup never reaches the page.

import { fitContain, iconFileProblem, isIconImage, type IconFileProblem } from "./programPrefs";

export const ICON_SIZE = 32;

export class IconFileError extends Error {
  constructor(readonly problem: Exclude<IconFileProblem, null> | "read") {
    super(problem);
  }
}

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") resolve(reader.result);
      else reject(new IconFileError("read"));
    };
    reader.onerror = () => {
      reject(new IconFileError("read"));
    };
    reader.readAsDataURL(file);
  });
}

function canvas(size: number): [HTMLCanvasElement, CanvasRenderingContext2D] {
  const element = document.createElement("canvas");
  element.width = size;
  element.height = size;
  const context = element.getContext("2d");
  if (!context) throw new IconFileError("read");
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  return [element, context];
}

/**
 * Reads a PNG or SVG file and returns `data:image/png;base64,...` at 32 × 32. Large images are
 * scaled down in halving steps, so fine lines do not fray.
 */
export async function fileToIcon(file: File): Promise<string> {
  const problem = iconFileProblem(file.type, file.size);
  if (problem) throw new IconFileError(problem);
  const image = new Image();
  image.src = await readAsDataUrl(file);
  try {
    await image.decode();
  } catch {
    throw new IconFileError("read");
  }
  let source: CanvasImageSource = image;
  const width = image.naturalWidth || ICON_SIZE * 4;
  const height = image.naturalHeight || ICON_SIZE * 4;
  // First fit into a square of at most 512 px, then halve until just above 32 px.
  let size = Math.min(512, Math.max(width, height, ICON_SIZE));
  const [first, firstContext] = canvas(size);
  const box = fitContain(width, height, size);
  firstContext.drawImage(source, box.x, box.y, box.width, box.height);
  source = first;
  while (size / 2 >= ICON_SIZE) {
    size = Math.round(size / 2);
    const [next, nextContext] = canvas(size);
    nextContext.drawImage(source, 0, 0, size, size);
    source = next;
  }
  const [result, context] = canvas(ICON_SIZE);
  context.drawImage(source, 0, 0, ICON_SIZE, ICON_SIZE);
  let url: string;
  try {
    url = result.toDataURL("image/png");
  } catch {
    throw new IconFileError("read");
  }
  if (!isIconImage(url)) throw new IconFileError("read");
  return url;
}
