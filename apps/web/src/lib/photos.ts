/**
 * Wall photos, kept on this device in IndexedDB until accounts and cloud
 * storage exist. The project only holds each photo's id and size.
 */

const DATABASE = 'wain';
const STORE = 'photos';
/** Longest side of a stored photo. Plenty for a preview, and keeps storage small. */
const MAX_SIDE = 2048;

function open(): Promise<IDBDatabase> {
	return new Promise((resolve, reject) => {
		const request = indexedDB.open(DATABASE, 1);
		request.onupgradeneeded = () => request.result.createObjectStore(STORE);
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

async function withStore<T>(
	mode: IDBTransactionMode,
	action: (store: IDBObjectStore) => IDBRequest<T>
): Promise<T> {
	const database = await open();
	try {
		return await new Promise<T>((resolve, reject) => {
			const request = action(database.transaction(STORE, mode).objectStore(STORE));
			request.onsuccess = () => resolve(request.result);
			request.onerror = () => reject(request.error);
		});
	} finally {
		database.close();
	}
}

export function loadPhoto(id: string): Promise<Blob | undefined> {
	return withStore('readonly', (store) => store.get(id) as IDBRequest<Blob | undefined>);
}

export async function deletePhoto(id: string): Promise<void> {
	await withStore('readwrite', (store) => store.delete(id));
}

export type StoredPhoto = { image_id: string; width: number; height: number };

/**
 * Stores a photo the user picked and returns its id and pixel size.
 *
 * The image is redrawn at a sensible size and re-encoded as JPEG. That
 * applies the camera's rotation, and it also drops the original file's
 * metadata, including any GPS location.
 */
export async function importPhoto(file: Blob): Promise<StoredPhoto> {
	const bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
	const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
	const width = Math.max(1, Math.round(bitmap.width * scale));
	const height = Math.max(1, Math.round(bitmap.height * scale));

	const canvas = document.createElement('canvas');
	canvas.width = width;
	canvas.height = height;
	const context = canvas.getContext('2d');
	if (!context) throw new Error('This browser cannot process images.');
	context.drawImage(bitmap, 0, 0, width, height);
	bitmap.close();

	const blob = await new Promise<Blob>((resolve, reject) => {
		canvas.toBlob(
			(result) => (result ? resolve(result) : reject(new Error('Could not encode the photo.'))),
			'image/jpeg',
			0.85
		);
	});

	const image_id = crypto.randomUUID();
	await withStore('readwrite', (store) => store.put(blob, image_id));
	return { image_id, width, height };
}
