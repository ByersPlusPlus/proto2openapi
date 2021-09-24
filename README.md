# proto2openapi

A nifty little tool to generate a full OpenAPI specification from a proto file with only little additional effort!

## How to use

Edit your protobuf file(s) to include comments over those methods you wish to include in the specification like this:

```protobuf
syntax = "proto3";
package helloworld;
import "google/protobuf/empty.proto";

service HelloService {
    // GET /hello [Hello]
    rpc SayHello (google.protobuf.Empty) returns (CommandList);
}

message HelloMessage {
    string message = 1;
}
```

Then run `proto2openapi ./openapi.yaml --title "Hello World" --version "1.0.0" -p helloworld.proto`. This will generate a file called `openapi.yaml` in your current directory, which contains the OpenAPI specification!

## Documentation of the comments

A method comment always needs at least a method (GET, PUT, POST and DELETE are currently supported) and a path specification (like /users).

If you want to include parameters into your path, you can include them like this: `GET /users/{userId:int}`. A parameter pair like this can either have the type `string` or `int`.

By default, proto2openapi converts the input type of the method to the request body (except on GET requests, where a body is not supported). If you want to omit a request body entirely (like on DELETE functions), add a `- BODY` to the comment like `DELETE /users/{userId:int} - BODY`.

Lastly, if you want to organize methods, you can add tags to the comment like this `GET /groups/{groupId:int} - BODY [Groups, Some other tag]`. Tags are seperated by comma.

## Afterword

This tool is not really meant as a general purpose tool. It was created out of laziness, because ByersPlusPlus needed an API gateway which was automatically generated. This way, we don't have to write the OpenAPI specification ourselves and we can generate a server stub automatically, which can then be implemented, either by hand or automatically as well.

If you wish to improve on this, feel free to.
