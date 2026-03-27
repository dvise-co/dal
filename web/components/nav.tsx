"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const links = [
  { href: "/queue", label: "Queue" },
  { href: "/review", label: "Review" },
];

export default function Nav() {
  const pathname = usePathname();

  return (
    <header className="border-b border-border bg-zinc-900">
      <div className="max-w-6xl mx-auto px-4 h-14 flex items-center gap-8">
        <Link href="/" className="text-lg font-bold tracking-tight">
          DAL
        </Link>
        <nav className="flex gap-1">
          {links.map(({ href, label }) => (
            <Link
              key={href}
              href={href}
              className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors ${
                pathname === href
                  ? "bg-zinc-800 text-white"
                  : "text-muted-foreground hover:text-white hover:bg-zinc-800/50"
              }`}
            >
              {label}
            </Link>
          ))}
        </nav>
      </div>
    </header>
  );
}
