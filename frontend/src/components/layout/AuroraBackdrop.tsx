export function AuroraBackdrop() {
  return (
    <div aria-hidden="true" className="pointer-events-none fixed inset-0 -z-10 overflow-hidden bg-void">
      <img
        src="/aurora-bg.svg"
        alt=""
        className="absolute inset-0 size-full scale-110 object-cover opacity-80"
      />
      {/* Heavy dark blur keeps the aurora atmospheric while text stays legible. */}
      <div className="absolute inset-0 bg-black/50 backdrop-blur-xl" />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top,transparent_0%,rgba(5,6,10,0.55)_65%,rgba(5,6,10,0.92)_100%)]" />
      <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-neon-emerald/40 to-transparent" />
    </div>
  )
}
