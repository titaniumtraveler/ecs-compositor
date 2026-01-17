# Asynchronous and Parallel Multi-Producer Single-Consumer/Single-Producer Multi-Consumer Ring Queue

## Assumptions

This data structure is made for making zero-copy implementations of protocols that look
roughly like this:

There is a stream of unstructured data that is being read from and written to.

There is one reader, which reads a chunk of data from the stream,
parses that data into frames and gives out handles to these frames to the
consumer tasks, which process the messages and then free them out of order.
(Single producer, multiple Consumer)

There are multiple producers, that allocate some (fixed size!) space,
take some time to write the message content into their allocated space and then
commit that buffer out of order. All the committed content is then at some point
taken by the writer and written to the stream.
(Multi-Producer, single Consumer)

Both the reader and the writer role can be shared by protecting it with a mutex.
In that case there is not separate reader/writer task, but the role is shared
and the work is done my the first task locking the mutex after new work is to be
done.

The queue is implemented as ring buffer (more precisely [BipBuffer]), where in
the reader case, the "empty" space between `next..free` is reserved for
the reader and in the writer case, the committed space between `free..commit` is
reserved for the writer.

While the shared resource that a ring buffer protects, is *normally* a range of
bytes, this structure can *theoretically* be used to manage access to
*arbitrary* types of resources.
For example the wayland protocol (for which I am implementing this in the first place)
isn't only used to send and receive bytes, but can also send and receive file
descriptors in the form of auxiliary data and those have to be handled in a
separate buffer, but as they are sent in the exact same order as the data, both
can share the same ring queue.

[BipBuffer]: https://www.codeproject.com/articles/The-Bip-Buffer-The-Circular-Buffer-with-a-Twist#comments-section
