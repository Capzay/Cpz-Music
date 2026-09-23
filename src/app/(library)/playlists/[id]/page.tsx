import { PlaylistsApp } from "@/components/PlaylistsApp";

export default async function PlaylistPage(props: PageProps<"/playlists/[id]">) {
  const { id } = await props.params;
  return <PlaylistsApp routeId={id} />;
}
