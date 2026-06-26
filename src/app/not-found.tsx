import type { Metadata } from 'next';
import Link from 'next/link';

export const metadata: Metadata = {
  title: 'Page not found | Parashield Protocol',
  description: "The page you're looking for doesn't exist.",
};

export default function NotFound() {
  return (
    <main className="flex min-h-[calc(100vh-128px)] flex-col items-center justify-center text-center px-6">
      <p className="text-8xl font-black text-teal-500/20">404</p>
      <h1 className="mt-4 text-2xl font-bold text-white">Page not found</h1>
      <p className="mt-2 text-gray-400">The page you&apos;re looking for doesn&apos;t exist.</p>
      <Link
        href="/"
        className="mt-8 rounded-xl bg-teal-500 px-6 py-2.5 font-semibold text-white hover:bg-teal-400 transition-colors"
      >
        Back to products
      </Link>
    </main>
  );
}
