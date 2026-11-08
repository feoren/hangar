// Draws the overlay icon for the taskbar badge (Windows) as PNG bytes: a round disc in the
// attention color with the count. Windows scales overlay icons to the small icon size (16 px at
// 100 %), so it is drawn at 32 px for high DPI and kept to one or two characters.

const SIZE = 32;
/** Dark ring so a light disc stays visible on a light taskbar and vice versa. */
const RING = "#1b1d22";

/** Relative luminance of `#rrggbb` (WCAG); 0 for anything else. */
function luminance(color: string): number {
  const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color);
  if (!match) return 0;
  const [r, g, b] = match.slice(1).map((hex) => {
    const value = parseInt(hex, 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
}

export async function drawBadgePng(text: string, color: string): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = SIZE;
  canvas.height = SIZE;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("No 2D canvas");
  context.beginPath();
  context.arc(SIZE / 2, SIZE / 2, SIZE / 2 - 1, 0, Math.PI * 2);
  context.fillStyle = RING;
  context.fill();
  context.beginPath();
  context.arc(SIZE / 2, SIZE / 2, SIZE / 2 - 3, 0, Math.PI * 2);
  context.fillStyle = color;
  context.fill();
  context.fillStyle = luminance(color) > 0.4 ? RING : "#ffffff";
  context.font = `bold ${String(text.length > 1 ? 15 : 19)}px "Segoe UI", system-ui, sans-serif`;
  context.textAlign = "center";
  context.textBaseline = "middle";
  context.fillText(text, SIZE / 2, SIZE / 2 + 1);
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, "image/png");
  });
  if (!blob) throw new Error("Badge image failed");
  return new Uint8Array(await blob.arrayBuffer());
}
