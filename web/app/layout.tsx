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
      <body className="min-h-full flex flex-col bg-background text-foreground font-sans antialiased">
        <Nav />
        <main className="flex-1 max-w-screen-2xl mx-auto w-full px-6 py-4">
          {children}
        </main>
      </body>
    </html>
  );
}
