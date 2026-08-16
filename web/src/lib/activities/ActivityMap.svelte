<script lang="ts">
  import type { ActivityPhotos, ActivityTrack } from '../api/types';
  interface Props { track: ActivityTrack; photos: ActivityPhotos | null }
  let { track, photos }: Props = $props();
  const zoom = 14, tile = 256;
  let element = $state<HTMLDivElement>(); let width = $state(720); let height = $state(380);
  $effect(() => { if (!element || typeof ResizeObserver === 'undefined') return; const observer = new ResizeObserver(() => { width = element?.clientWidth || width; height = element?.clientHeight || height; }); observer.observe(element); return () => observer.disconnect(); });
  function project(lat: number, lon: number) { const bounded = Math.max(-85.0511, Math.min(85.0511, lat)); const s = Math.sin(bounded * Math.PI / 180), world = tile * 2 ** zoom; return { x: (lon + 180) / 360 * world, y: (0.5 - Math.log((1+s)/(1-s))/(4*Math.PI))*world }; }
  function center() { const points = track.points.map((p) => project(p.lat, p.lon)); return { x: points.reduce((n,p)=>n+p.x,0)/points.length, y: points.reduce((n,p)=>n+p.y,0)/points.length }; }
  function position(lat: number, lon: number) { const p = project(lat, lon), c = center(); return { left: p.x-c.x+width/2, top: p.y-c.y+height/2 }; }
  function tiles() { const c = center(), n = 2 ** zoom, result: Array<{x:number;y:number;left:number;top:number}>=[]; for(let y=Math.max(0,Math.floor((c.y-height/2)/tile));y<=Math.min(n-1,Math.floor((c.y+height/2)/tile));y++) for(let x=Math.max(0,Math.floor((c.x-width/2)/tile));x<=Math.min(n-1,Math.floor((c.x+width/2)/tile));x++) result.push({x,y,left:x*tile-c.x+width/2,top:y*tile-c.y+height/2}); return result; }
  function route() { return track.points.map((p) => { const q = position(p.lat,p.lon); return `${q.left},${q.top}`; }).join(' '); }
</script>
<div class="map" bind:this={element} role="img" aria-label={`OpenStreetMap 路线地图，包含 ${track.original_count} 个轨迹点和 ${photos?.total ?? 0} 张活动照片`}>
  {#each tiles() as item (`${item.x}:${item.y}`)}<img class="tile" src={`https://tile.openstreetmap.org/${zoom}/${item.x}/${item.y}.png`} alt="" style:left={`${item.left}px`} style:top={`${item.top}px`} />{/each}
  <svg aria-hidden="true" viewBox={`0 0 ${width} ${height}`}><polyline points={route()} /></svg>
  {#each photos?.photos ?? [] as photo (photo.id)}{#if photo.gps_lat !== null && photo.gps_lon !== null}{@const point = position(photo.gps_lat, photo.gps_lon)}<img class="photo" src={`/api/photos/${photo.id}/thumb?size=128`} alt={`活动地图照片 ${photo.id}`} style:left={`${point.left}px`} style:top={`${point.top}px`} />{/if}{/each}
  <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noreferrer">© OpenStreetMap contributors</a>
</div>
<style>.map{position:relative;min-height:350px;overflow:hidden;border-radius:20px;background:#b8d8e8}.tile{position:absolute;width:256px;height:256px;max-width:none}.map svg{position:absolute;inset:0;width:100%;height:100%;pointer-events:none}.map polyline{fill:none;stroke:#1769e0;stroke-width:6;stroke-linecap:round;stroke-linejoin:round;filter:drop-shadow(0 1px 2px white)}.photo{position:absolute;z-index:2;width:38px;height:38px;border:3px solid white;border-radius:50%;object-fit:cover;transform:translate(-50%,-50%);box-shadow:0 2px 8px #0008}.map a{position:absolute;right:8px;bottom:6px;z-index:3;padding:2px 4px;color:#333;background:#fffd;font-size:10px}</style>
