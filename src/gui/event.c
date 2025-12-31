/* Event handling - pure event queue implementation
   This file contains only the event queue circular buffer.
   Event loop (XCB integration) is in event_loop.c */

#include <stdlib.h>
#include <string.h>
#include "silksurf/events.h"

/* (Event queue implementation is in this file)
   The event queue circular buffer implementation is in events.c
   This file is kept for backward compatibility and potential
   event filtering/transformation logic. */
