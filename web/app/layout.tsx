import type { Metadata } from "next";
import Nav from "@/components/nav";
import "./globals.css";

export const metadata: Metadata = {
  title: "DAL — Download, Assess, Label",
  description: "Self-hosted content review pipeline",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="h-full dark">
      <body className="min-h-full flex flex-col bg-zinc-950 text-zinc-100 font-sans">
        <Nav />
        <main className="flex-1 max-w-6xl mx-auto w-full px-4 py-6">
          {children}
        </main>
      </body>
    </html>
  );
}
