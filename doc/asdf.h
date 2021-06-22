#ifndef ASDF_H
#define ASDF_H

/* Generated with cbindgen:0.15.0 */


#define ASDF_MAJOR 0
#define ASDF_MINOR 0
#define ASDF_PATCH 0


#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum {
  ASDF_STREAMING_SUCCESS,
  ASDF_STREAMING_EMPTY_BUFFER,
  ASDF_STREAMING_INCOMPLETE_SEEK,
  ASDF_STREAMING_SEEK_WHILE_ROLLING,
} AsdfStreamingResult;

typedef struct AsdfScene AsdfScene;

/**
 * Static information about a source.
 *
 * Use asdf_get_source_transform() to get dynamic information
 * about a source at a given frame.
 */
typedef struct {
  const char *id;
  const char *name;
  const char *model;
  const char *port;
} AsdfSourceInfo;

typedef struct {
  bool active;
  float pos[3];
  /**
   * Vector part of quaternion
   */
  float rot_v[3];
  /**
   * Scalar part of quaternion
   */
  float rot_s;
  float vol;
} AsdfTransform;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Load an ASDF scene from a file.
 *
 * Before starting playback (i.e. calling asdf_get_audio_data()
 * with `rolling` set to `true`), asdf_seek() has to be called.
 *
 * The returned object must be discarded with asdf_scene_free().
 *
 * In case of an error, NULL is returned and
 * asdf_last_error() can be used to get an error description.
 */
AsdfScene *asdf_scene_new(const char *filename,
                          uint32_t samplerate,
                          uint32_t blocksize,
                          uint32_t buffer_blocks,
                          uint64_t usleeptime);

/**
 * Discard a scene object created with asdf_scene_new().
 *
 * Passing NULL is allowed.
 */
void asdf_scene_free(AsdfScene*);

/**
 * Get number of file sources.
 */
uint32_t asdf_file_sources(const AsdfScene *scene);

/**
 * Get number of live sources.
 */
uint32_t asdf_live_sources(const AsdfScene *scene);

/**
 * Get scene duration in frames.
 *
 * Returns `0` if the duration is undefined.
 */
uint64_t asdf_frames(const AsdfScene *scene);

/**
 * Get an AsdfSourceInfo object for a given (0-based) source index.
 *
 * Calling this function with an invalid source index invokes undefined behavior.
 *
 * The returned object must be discarded with asdf_sourceinfo_free().
 */
AsdfSourceInfo *asdf_get_sourceinfo(const AsdfScene *scene, uint32_t source_index);

/**
 * Discard a source object created with asdf_get_sourceinfo().
 */
void asdf_sourceinfo_free(AsdfSourceInfo*);

/**
 * Get AsdfTransform for a given (0-based) source index at a given frame.
 *
 * Calling this function with an invalid source index invokes undefined behavior.
 *
 * This function is realtime-safe.
 */
AsdfTransform asdf_get_source_transform(const AsdfScene *scene,
                                        uint32_t source_index,
                                        uint64_t frame);

/**
 * Get AsdfTransform for the reference at a given frame.
 *
 * The reference transform is always "active".
 *
 * This function is realtime-safe.
 */
AsdfTransform asdf_get_reference_transform(const AsdfScene *scene, uint64_t frame);

/**
 * Seek to the given frame.
 *
 * Returns `true` when seeking has completed.  If `false` is returned,
 * the function has to be called again at a later time
 * until `true` is returned.
 *
 * While seeking, it is not allowed to call asdf_get_audio_data()
 * with the `rolling` argument set to `true`.
 *
 * A return value of `false` doesn't mean an error occured,
 * therefore asdf_last_error() will not contain relevant information.
 *
 * This function is realtime-safe.
 */
bool asdf_seek(AsdfScene *scene, uint64_t frame);

/**
 * Get a block of audio data.
 *
 * If `rolling` is `false`, `data` will be filled with zeros.
 * Before being able to call this function with `rolling` set to `true`,
 * asdf_seek() has to be called (potentially repeatedly, until it returns `true`).
 *
 * In case of an error, `data` will be filled with zeros.
 *
 * `data` is only allowed to be NULL when there are no file sources.
 *
 * This function is realtime-safe but not re-entrant.
 */
AsdfStreamingResult asdf_get_audio_data(AsdfScene *scene, float *const *data, bool rolling);

/**
 * Obtain the error message of the last error.
 *
 * The error message will be freed if another error occurs. It is the caller's
 * responsibility to make sure they're no longer using the string before
 * calling any other function which may fail.
 *
 * The error message is thread-local, i.e. it can only be obtained
 * from the thread on which the error occured.
 */
const char *asdf_last_error(void);

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus

#endif /* ASDF_H */
