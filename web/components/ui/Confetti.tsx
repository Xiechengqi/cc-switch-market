type ConfettiProps = {
  density?: "low" | "medium";
};

const SHAPES_LOW = [
  { type: "circle", x: "8%", y: "10%", color: "#fbbf24", size: 18, rot: 0 },
  { type: "square", x: "92%", y: "18%", color: "#f472b6", size: 14, rot: 18 },
  { type: "triangle", x: "10%", y: "80%", color: "#34d399", size: 22, rot: 0 },
  { type: "circle", x: "88%", y: "70%", color: "#8b5cf6", size: 12, rot: 0 }
];

const SHAPES_MED = [
  ...SHAPES_LOW,
  { type: "square", x: "30%", y: "14%", color: "#34d399", size: 10, rot: 30 },
  { type: "circle", x: "60%", y: "82%", color: "#f472b6", size: 16, rot: 0 },
  { type: "triangle", x: "75%", y: "30%", color: "#fbbf24", size: 14, rot: -10 },
  { type: "square", x: "20%", y: "60%", color: "#8b5cf6", size: 12, rot: -20 }
];

export function Confetti({ density = "low" }: ConfettiProps) {
  const shapes = density === "medium" ? SHAPES_MED : SHAPES_LOW;
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      {shapes.map((s, i) => (
        <div
          key={i}
          className="absolute animate-float"
          style={{
            left: s.x,
            top: s.y,
            transform: `rotate(${s.rot}deg)`,
            animationDelay: `${i * 0.4}s`
          }}
        >
          {s.type === "circle" && <div style={{ width: s.size, height: s.size, background: s.color, border: "2px solid #1e293b", borderRadius: 999 }} />}
          {s.type === "square" && <div style={{ width: s.size, height: s.size, background: s.color, border: "2px solid #1e293b", borderRadius: 4 }} />}
          {s.type === "triangle" && (
            <svg width={s.size} height={s.size} viewBox="0 0 24 24"><polygon points="12,3 22,21 2,21" fill={s.color} stroke="#1e293b" strokeWidth={2} strokeLinejoin="round" /></svg>
          )}
        </div>
      ))}
    </div>
  );
}
