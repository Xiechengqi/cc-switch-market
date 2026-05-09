type LogoProps = { size?: number; className?: string; withConfetti?: boolean };

export function MarketLogo({ size = 40, className, withConfetti = true }: LogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 80 80"
      className={className}
      role="img"
      aria-label="cc-switch Market"
      xmlns="http://www.w3.org/2000/svg"
    >
      <circle cx="44" cy="44" r="30" fill="#1E293B" />
      <circle cx="43" cy="43" r="30" fill="#FBBF24" stroke="#1E293B" strokeWidth={2} />
      <circle cx="40" cy="40" r="30" fill="#8B5CF6" stroke="#1E293B" strokeWidth={2} />
      <text
        x="40"
        y="52"
        textAnchor="middle"
        fontFamily="Outfit, system-ui, sans-serif"
        fontWeight={800}
        fontSize={36}
        fill="#FFFFFF"
      >
        M
      </text>
      {withConfetti && (
        <polygon
          points="68,8 76,22 60,22"
          fill="#F472B6"
          stroke="#1E293B"
          strokeWidth={2}
          transform="rotate(18 68 15)"
        />
      )}
    </svg>
  );
}

export function RouterLogo({ size = 40, className }: LogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 80 80"
      className={className}
      role="img"
      aria-label="Switch Router"
      xmlns="http://www.w3.org/2000/svg"
    >
      <line x1="40" y1="22" x2="40" y2="8" stroke="#1E293B" strokeWidth={2.5} strokeLinecap="round" />
      <line x1="58" y1="40" x2="72" y2="40" stroke="#1E293B" strokeWidth={2.5} strokeLinecap="round" />
      <line x1="40" y1="58" x2="40" y2="72" stroke="#1E293B" strokeWidth={2.5} strokeLinecap="round" />
      <line x1="22" y1="40" x2="8" y2="40" stroke="#1E293B" strokeWidth={2.5} strokeLinecap="round" />
      <circle cx="40" cy="6" r="6" fill="#8B5CF6" stroke="#1E293B" strokeWidth={2} />
      <circle cx="74" cy="40" r="6" fill="#F472B6" stroke="#1E293B" strokeWidth={2} />
      <circle cx="40" cy="74" r="6" fill="#FBBF24" stroke="#1E293B" strokeWidth={2} />
      <circle cx="6" cy="40" r="6" fill="#10B981" stroke="#1E293B" strokeWidth={2} />
      <circle cx="43" cy="43" r="18" fill="#1E293B" />
      <circle cx="40" cy="40" r="18" fill="#34D399" stroke="#1E293B" strokeWidth={2} />
      <text
        x="40"
        y="48"
        textAnchor="middle"
        fontFamily="Outfit, system-ui, sans-serif"
        fontWeight={800}
        fontSize={22}
        fill="#FFFFFF"
      >
        R
      </text>
    </svg>
  );
}
